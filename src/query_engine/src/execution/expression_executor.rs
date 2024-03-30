use arrow::array::ArrayRef;
use arrow::datatypes::DataType;
use arrow::record_batch::RecordBatch;

use super::{ExecutorError, RecordBatchUtil};
use crate::planner::BoundExpression;
use crate::types::ScalarValue;

/// ExpressionExecutor is responsible for executing a set of expressions and storing the result in a
/// data chunk
pub struct ExpressionExecutor;

impl ExpressionExecutor {
    pub fn execute(
        expressions: &[BoundExpression],
        input: &RecordBatch,
    ) -> Result<Vec<ArrayRef>, ExecutorError> {
        let mut result = vec![];
        for expr in expressions.iter() {
            result.push(Self::execute_internal(expr, input)?);
        }
        Ok(result)
    }

    pub fn execute_scalar(expression: &BoundExpression) -> Result<ScalarValue, ExecutorError> {
        let input = RecordBatchUtil::new_one_row_dummy_batch()?;
        let res = Self::execute(&[expression.clone()], &input)?;
        assert!(res.len() == 1);
        let col = res.first().expect("no first");
        assert_eq!(DataType::from(expression.return_type()), *col.data_type());
        let val = ScalarValue::try_from_array(col, 0)?;
        Ok(val)
    }

    fn execute_internal(
        expr: &BoundExpression,
        input: &RecordBatch,
    ) -> Result<ArrayRef, ExecutorError> {
        Ok(match expr {
            BoundExpression::BoundColumnRefExpression(_) => todo!(),
            BoundExpression::BoundConstantExpression(e) => {
                e.value.to_array_of_size(input.num_rows())
            }
            BoundExpression::BoundReferenceExpression(e) => input.column(e.index).clone(),
            BoundExpression::BoundCastExpression(e) => {
                let child_result = Self::execute_internal(&e.child, input)?;
                let cast_function = e.function.function;
                cast_function(&child_result, &e.base.return_type, e.try_cast)?
            }
            BoundExpression::BoundFunctionExpression(e) => {
                let children_result = e
                    .children
                    .iter()
                    .map(|c| Self::execute_internal(c, input))
                    .collect::<Result<Vec<_>, _>>()?;
                let func = e.function.function;
                func(&children_result)?
            }
            BoundExpression::BoundComparisonExpression(e) => {
                let left_result = Self::execute_internal(&e.left, input)?;
                let right_result = Self::execute_internal(&e.right, input)?;
                let func = e.function.function;
                func(&left_result, &right_result)?
            }
            BoundExpression::BoundConjunctionExpression(e) => {
                assert!(e.children.len() >= 2);
                let mut conjunction_result = Self::execute_internal(&e.children[0], input)?;
                for i in 1..e.children.len() {
                    let func = e.function.function;
                    conjunction_result = func(
                        &conjunction_result,
                        &Self::execute_internal(&e.children[i], input)?,
                    )?;
                }
                conjunction_result
            }
        })
    }
}
