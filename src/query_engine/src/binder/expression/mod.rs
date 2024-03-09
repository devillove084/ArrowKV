mod agg_func;
mod binary_op;
use std::{fmt, slice};

pub use agg_func::*;
use arrow::datatypes::DataType;
pub use binary_op::*;
use itertools::Itertools;
use paste::paste;
use sqlparser::ast::{Expr, Ident};

use super::{BindError, Binder, BoundSubqueryExpr};
use crate::catalog::{ColumnCatalog, ColumnId, TableId};
use crate::optimizer::ExprVisitor;
use crate::types::ScalarValue;

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum BoundExpr {
    Constant(ScalarValue),
    ColumnRef(BoundColumnRef),
    InputRef(BoundInputRef),
    BinaryOp(BoundBinaryOp),
    TypeCast(BoundTypeCast),
    AggFunc(BoundAggFunc),
    Alias(BoundAlias),
    Subquery(BoundSubqueryExpr),
}

impl BoundExpr {
    pub fn nullable(&self) -> bool {
        match self {
            BoundExpr::Constant(_) => false,
            BoundExpr::ColumnRef(e) => e.column_catalog.nullable,
            BoundExpr::InputRef(_) => unreachable!(),
            BoundExpr::BinaryOp(e) => e.left.nullable() && e.right.nullable(),
            BoundExpr::TypeCast(e) => e.expr.nullable(),
            BoundExpr::AggFunc(e) => e.exprs[0].nullable(),
            BoundExpr::Alias(e) => e.expr.nullable(),
            BoundExpr::Subquery(e) => e.query_ref.query.select_list[0].nullable(),
        }
    }

    pub fn return_type(&self) -> Option<DataType> {
        match self {
            BoundExpr::Constant(value) => Some(value.data_type()),
            BoundExpr::InputRef(input) => Some(input.return_type.clone()),
            BoundExpr::ColumnRef(column_ref) => {
                Some(column_ref.column_catalog.desc.data_type.clone())
            }
            BoundExpr::BinaryOp(binary_op) => binary_op.return_type.clone(),
            BoundExpr::TypeCast(tc) => Some(tc.cast_type.clone()),
            BoundExpr::AggFunc(agg) => Some(agg.return_type.clone()),
            BoundExpr::Alias(alias) => alias.expr.return_type(),
            BoundExpr::Subquery(e) => e.query_ref.query.select_list[0].return_type(),
        }
    }

    pub fn get_referenced_column_catalog(&self) -> Vec<ColumnCatalog> {
        match self {
            BoundExpr::Constant(_) => vec![],
            BoundExpr::InputRef(_) => vec![],
            BoundExpr::ColumnRef(column_ref) => vec![column_ref.column_catalog.clone()],
            BoundExpr::BinaryOp(binary_op) => binary_op
                .left
                .get_referenced_column_catalog()
                .into_iter()
                .chain(binary_op.right.get_referenced_column_catalog())
                .collect::<Vec<_>>(),
            BoundExpr::TypeCast(tc) => tc.expr.get_referenced_column_catalog(),
            BoundExpr::AggFunc(agg) => agg
                .exprs
                .iter()
                .flat_map(|arg| arg.get_referenced_column_catalog())
                .collect::<Vec<_>>(),
            BoundExpr::Alias(alias) => alias.expr.get_referenced_column_catalog(),
            BoundExpr::Subquery(_) => unreachable!(),
        }
    }

    /// Generate a new column catalog for this expression.
    /// Such as `t.v` in subquery: select t.v from (select a as v from t1) t.
    /// Constant and BinaryOp returns empty table_id.
    pub fn output_column_catalog(&self) -> ColumnCatalog {
        let (table_id, column_id, data_type) = match self {
            BoundExpr::Constant(e) => (String::new(), e.to_string(), e.data_type()),
            BoundExpr::ColumnRef(e) => (
                e.column_catalog.table_id.clone(),
                e.column_catalog.column_id.clone(),
                e.column_catalog.desc.data_type.clone(),
            ),
            BoundExpr::InputRef(_) => unreachable!(),
            BoundExpr::BinaryOp(e) => {
                let l = e.left.output_column_catalog();
                let r = e.right.output_column_catalog();
                let column_id = format!("{}{}{}", l.column_id, e.op, r.column_id);
                let data_type = e.return_type.clone().unwrap();
                (String::new(), column_id, data_type)
            }
            BoundExpr::TypeCast(e) => {
                let c = e.expr.output_column_catalog();
                let table_id = c.table_id;
                let column_id = format!("{}({})", e.cast_type, c.column_id);
                let data_type = e.cast_type.clone();
                (table_id, column_id, data_type)
            }
            BoundExpr::AggFunc(agg) => {
                let c = agg.exprs[0].output_column_catalog();
                let table_id = c.table_id;
                let column_id = format!("{}({})", agg.func, c.column_id);
                let data_type = agg.return_type.clone();
                (table_id, column_id, data_type)
            }
            BoundExpr::Alias(e) => {
                let table_id = e.table_id.clone();
                let column_id = e.column_id.to_string();
                let data_type = e.expr.return_type().unwrap();
                (table_id, column_id, data_type)
            }
            BoundExpr::Subquery(_) => unreachable!(),
        };
        ColumnCatalog::new(table_id, column_id, self.nullable(), data_type)
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BoundColumnRef {
    pub column_catalog: ColumnCatalog,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BoundInputRef {
    /// column index in data chunk
    pub index: usize,
    pub return_type: DataType,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BoundTypeCast {
    /// original expression
    pub expr: Box<BoundExpr>,
    pub cast_type: DataType,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct BoundAlias {
    pub expr: Box<BoundExpr>,
    pub column_id: ColumnId,
    pub table_id: TableId,
}

impl Binder {
    /// bind sqlparser Expr into BoundExpr
    pub fn bind_expr(&mut self, expr: &Expr) -> Result<BoundExpr, BindError> {
        match expr {
            Expr::Identifier(ident) => {
                self.bind_column_ref_from_identifiers(slice::from_ref(ident))
            }
            Expr::CompoundIdentifier(idents) => self.bind_column_ref_from_identifiers(idents),
            Expr::BinaryOp { left, op, right } => self.bind_binary_op(left, op, right),
            Expr::UnaryOp { op: _, expr: _ } => todo!(),
            Expr::Value(v) => Ok(BoundExpr::Constant(v.into())),
            Expr::Function(func) => self.bind_agg_func(func),
            Expr::Nested(expr) => self.bind_expr(expr),
            Expr::Subquery(query) => self.bind_scalar_subquery(query),
            _ => todo!("unsupported expr {:?}", expr),
        }
    }

    /// bind sqlparser Identifier into BoundExpr
    ///
    /// Identifier types:
    ///  * Identifier(Ident): Identifier e.g. table name or column name
    ///  * CompoundIdentifier(Vec<Ident>): Multi-part identifier, e.g. `table_alias.column` or
    ///    `schema.table.col`
    ///
    /// so, the idents slice could be `[col]`, `[table, col]` or `[schema, table, col]`
    pub fn bind_column_ref_from_identifiers(
        &mut self,
        idents: &[Ident],
    ) -> Result<BoundExpr, BindError> {
        let idents = idents
            .iter()
            .map(|ident| Ident::new(ident.value.to_lowercase()))
            .collect_vec();

        let (_schema_name, table_name, column_name) = match idents.as_slice() {
            [column] => (None, None, &column.value),
            [table, column] => (None, Some(&table.value), &column.value),
            [schema, table, column] => (Some(&schema.value), Some(&table.value), &column.value),
            _ => return Err(BindError::InvalidTableName(idents)),
        };

        if let Some(table) = table_name {
            // handle table.col syntax
            let table_catalog = self.context.tables.get(table).ok_or_else(|| {
                println!("InvalidTable in context: {:#?}", self.context);
                BindError::InvalidTable(table.clone())
            })?;
            let column_catalog =
                table_catalog
                    .get_column_by_name(column_name)
                    .ok_or_else(|| {
                        println!("InvalidColumn in context: {:#?}", self.context);
                        BindError::InvalidColumn(column_name.clone())
                    })?;
            Ok(BoundExpr::ColumnRef(BoundColumnRef { column_catalog }))
        } else {
            // handle col syntax
            let mut got_column = None;
            for table_catalog in self.context.tables.values() {
                if let Some(column_catalog) = table_catalog.get_column_by_name(column_name) {
                    // ambiguous column check
                    if got_column.is_some() {
                        return Err(BindError::AmbiguousColumn(column_name.clone()));
                    }
                    got_column = Some(column_catalog);
                }
            }
            // handle col alias
            if got_column.is_none() {
                if let Some(expr) = self.context.aliases.get(column_name) {
                    return Ok(expr.clone());
                }
            }
            let column_catalog = got_column.ok_or_else(|| {
                println!("InvalidColumn in context: {:#?}", self.context);
                BindError::InvalidColumn(column_name.clone())
            })?;
            Ok(BoundExpr::ColumnRef(BoundColumnRef { column_catalog }))
        }
    }
}

impl fmt::Debug for BoundExpr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            BoundExpr::Constant(value) => write!(f, "{}", value),
            BoundExpr::ColumnRef(column_ref) => write!(f, "{:?}", column_ref),
            BoundExpr::InputRef(input_ref) => write!(f, "{:?}", input_ref),
            BoundExpr::BinaryOp(binary_op) => write!(f, "{:?}", binary_op),
            BoundExpr::TypeCast(type_cast) => write!(f, "{:?}", type_cast),
            BoundExpr::AggFunc(agg_func) => write!(f, "{:?}", agg_func),
            BoundExpr::Alias(alias) => {
                write!(
                    f,
                    "({:?}) as {}.{}",
                    alias.expr, alias.table_id, alias.column_id
                )
            }
            BoundExpr::Subquery(subquery) => {
                write!(f, "ScalarSubquery {{{:?}}}", subquery.query_ref)
            }
        }
    }
}

impl fmt::Debug for BoundColumnRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.column_catalog)
    }
}

impl fmt::Debug for BoundInputRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "InputRef#{}:{}", self.index, self.return_type)
    }
}

impl fmt::Debug for BoundTypeCast {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cast({:?} as {})", self.expr, self.cast_type)
    }
}

macro_rules! impl_contains_variant {
    ( $($variant:ty),* ) => {
        paste! {
            impl BoundExpr {
                $(
                    pub fn [<contains_$variant:snake>](&self) -> bool {
                        struct Contains(bool);

                        impl ExprVisitor for Contains {
                            fn pre_visit(&mut self, expr: &BoundExpr) {
                                if let BoundExpr::$variant(_) = expr {
                                    self.0 = true;
                                }
                            }

                        }

                        let mut visitor = Contains(false);
                        visitor.visit_expr(self);
                        visitor.0
                    }
                )*
            }
        }
    };
}

impl_contains_variant! {Subquery}
