#[derive(Debug, Clone)]
pub enum ArithBiOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Debug, PartialEq, Clone)]
pub enum BoolUOp {
    Not,
}

#[derive(Debug, PartialEq, Clone)]
pub enum BoolBiOp {
    And,
    Or,
}

#[derive(Debug, Clone)]
pub enum ComOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}
