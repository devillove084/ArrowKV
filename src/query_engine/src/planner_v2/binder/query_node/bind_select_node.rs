use std::collections::HashMap;

use derive_new::new;
use sqlparser::ast::{Ident, SetExpr};

use super::BoundResultModifier;
use crate::planner_v2::{
    BindError, Binder, BoundExpression, BoundTableRef, ColumnAliasData, ExpressionBinder,
    SqlparserResolver, VALUES_LIST_ALIAS,
};
use crate::types_v2::LogicalType;

#[derive(new, Debug)]
pub struct BoundSelectNode {
    /// The names returned by this QueryNode.
    pub(crate) names: Vec<String>,
    /// The types returned by this QueryNode.
    pub(crate) types: Vec<LogicalType>,
    /// The projection list
    pub(crate) select_list: Vec<BoundExpression>,
    /// The FROM clause
    pub(crate) from_table: BoundTableRef,
    /// The WHERE clause
    pub(crate) where_clause: Option<BoundExpression>,
    /// The original unparsed expressions. This is exported after binding, because the binding
    /// might change the expressions (e.g. when a * clause is present)
    #[allow(dead_code)]
    pub(crate) original_select_items: Option<Vec<sqlparser::ast::Expr>>,
    /// Index used by the LogicalProjection
    #[new(default)]
    pub(crate) projection_index: usize,
    /// The result modifiers that should be applied to this query node
    pub(crate) modifiers: Vec<BoundResultModifier>,
}

#[allow(unused_variables)]
impl Binder {
    pub fn bind_query_body(&mut self, query_body: &SetExpr) -> Result<BoundSelectNode, BindError> {
        let projection_index = self.generate_table_index();
        let mut bound_select_node = match query_body {
            SetExpr::Select(select) => self.bind_select_body(select)?,
            SetExpr::Query(_) => todo!(),
            SetExpr::SetOperation {
                op,
                set_quantifier,
                left,
                right,
            } => todo!(),
            SetExpr::Values(v) => self.bind_values(v)?,
            SetExpr::Insert(_) => todo!(),
            SetExpr::Update(_) => todo!(),
            SetExpr::Table(_) => todo!(),
        };
        bound_select_node.projection_index = projection_index;
        Ok(bound_select_node)
    }

    pub fn bind_values(
        &mut self,
        values: &sqlparser::ast::Values,
    ) -> Result<BoundSelectNode, BindError> {
        let bound_expression_list_ref = self.bind_expression_list_ref(values)?;
        let names = bound_expression_list_ref.names.clone();
        let types = bound_expression_list_ref.types.clone();
        let mut expr_binder = ExpressionBinder::new(self);
        let select_list = names
            .iter()
            .map(|n| {
                let idents = vec![
                    Ident::new(VALUES_LIST_ALIAS.to_string()),
                    Ident::new(n.to_string()),
                ];
                expr_binder.bind_column_ref_expr(&idents, &mut vec![], &mut vec![])
            })
            .try_collect::<Vec<_>>()?;

        let bound_table_ref = BoundTableRef::BoundExpressionListRef(bound_expression_list_ref);
        let node = BoundSelectNode::new(
            names,
            types,
            select_list,
            bound_table_ref,
            None,
            None,
            vec![],
        );
        Ok(node)
    }

    pub fn bind_select_body(
        &mut self,
        select: &sqlparser::ast::Select,
    ) -> Result<BoundSelectNode, BindError> {
        // first bind the FROM table statement
        let from_table = self.bind_table_ref(select.from.as_slice())?;

        let mut result_names = vec![];
        let mut result_types = vec![];
        // expand any "*" statements
        let new_select_list = self.expand_star_expressions(select.projection.clone())?;
        if new_select_list.is_empty() {
            return Err(BindError::Internal("empty select list".to_string()));
        }

        // create a mapping of (alias -> index) and a mapping of (Expression -> index) for the
        // SELECT list
        let mut original_select_items = vec![];
        let mut alias_map = HashMap::new();
        for (idx, item) in new_select_list.iter().enumerate() {
            match item {
                sqlparser::ast::SelectItem::UnnamedExpr(expr) => {
                    original_select_items.push(expr.clone());
                }
                sqlparser::ast::SelectItem::ExprWithAlias { expr, alias } => {
                    alias_map.insert(alias.to_string(), idx);
                    original_select_items.push(expr.clone());
                }
                sqlparser::ast::SelectItem::Wildcard(..)
                | sqlparser::ast::SelectItem::QualifiedWildcard(..) => {
                    return Err(BindError::Internal(
                        "wildcard should be expanded before".to_string(),
                    ))
                }
            }
        }

        // first visit the WHERE clause
        // the WHERE clause happens before the GROUP BY, PROJECTION or HAVING clauses
        let where_clause = if let Some(where_expr) = &select.selection {
            let mut expr_binder = ExpressionBinder::new(self);
            expr_binder.set_column_alias_data(ColumnAliasData::new(
                original_select_items.clone(),
                alias_map.clone(),
            ));
            Some(expr_binder.bind_expression(where_expr, &mut vec![], &mut vec![])?)
        } else {
            None
        };

        let select_list = new_select_list
            .iter()
            .map(|item| self.bind_select_item(item, &mut result_names, &mut result_types))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(BoundSelectNode::new(
            result_names,
            result_types,
            select_list,
            from_table,
            where_clause,
            Some(original_select_items),
            vec![],
        ))
    }

    fn expand_star_expressions(
        &mut self,
        select_list: Vec<sqlparser::ast::SelectItem>,
    ) -> Result<Vec<sqlparser::ast::SelectItem>, BindError> {
        let mut new_select_list = vec![];
        for item in select_list {
            match item {
                sqlparser::ast::SelectItem::Wildcard(_) => {
                    let col_exprs = self.bind_context.generate_all_column_expressions(None)?;
                    new_select_list.extend(col_exprs);
                }
                sqlparser::ast::SelectItem::QualifiedWildcard(object_name, _) => {
                    let (_schema_name, table_name) =
                        SqlparserResolver::object_name_to_schema_table(&object_name)?;
                    let col_exprs = self
                        .bind_context
                        .generate_all_column_expressions(Some(table_name))?;
                    new_select_list.extend(col_exprs);
                }
                other => new_select_list.push(other),
            }
        }
        Ok(new_select_list)
    }

    fn bind_select_item(
        &mut self,
        item: &sqlparser::ast::SelectItem,
        result_names: &mut Vec<String>,
        result_types: &mut Vec<LogicalType>,
    ) -> Result<BoundExpression, BindError> {
        let mut expr_binder = ExpressionBinder::new(self);
        match item {
            sqlparser::ast::SelectItem::UnnamedExpr(expr) => {
                expr_binder.bind_expression(expr, result_names, result_types)
            }
            sqlparser::ast::SelectItem::ExprWithAlias { expr, alias } => {
                let mut expr = expr_binder.bind_expression(expr, result_names, result_types)?;
                expr.set_alias(alias.to_string());
                if let Some(last_name) = result_names.last_mut() {
                    *last_name = alias.to_string();
                }
                Ok(expr)
            }
            sqlparser::ast::SelectItem::Wildcard(..)
            | sqlparser::ast::SelectItem::QualifiedWildcard(..) => Err(BindError::Internal(
                "wildcard should expand before bind select item".to_string(),
            )),
        }
    }
}
