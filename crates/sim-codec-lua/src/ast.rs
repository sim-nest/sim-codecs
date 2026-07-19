use sim_kernel::Symbol;

/// A parsed Lua chunk or nested statement block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LuaBlock {
    /// Statements in source order.
    pub statements: Vec<LuaStmt>,
}

/// Lua statement node used by chunk decoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LuaStmt {
    /// A `local` declaration, including Lua 5.4 local attributes.
    Local {
        /// Declared names and optional attributes.
        bindings: Vec<LuaBinding>,
        /// Optional initializer expressions.
        values: Vec<LuaExpr>,
    },
    /// Assignment to one or more targets.
    Assign {
        /// Assignment targets.
        targets: Vec<LuaExpr>,
        /// Assigned values.
        values: Vec<LuaExpr>,
    },
    /// An `if` statement with zero or more `elseif` arms and an optional else.
    If {
        /// The `if` arm followed by any `elseif` arms.
        arms: Vec<LuaIfArm>,
        /// Optional `else` block.
        else_block: Option<LuaBlock>,
    },
    /// A `while` loop.
    While {
        /// Loop condition.
        condition: LuaExpr,
        /// Loop body.
        block: LuaBlock,
    },
    /// A `repeat ... until` loop.
    Repeat {
        /// Loop body.
        block: LuaBlock,
        /// Exit condition.
        condition: LuaExpr,
    },
    /// A numeric `for` loop.
    NumericFor {
        /// Loop variable name.
        name: Symbol,
        /// Initial expression.
        start: LuaExpr,
        /// Limit expression.
        limit: LuaExpr,
        /// Optional step expression.
        step: Option<LuaExpr>,
        /// Loop body.
        block: LuaBlock,
    },
    /// A generic `for ... in` loop.
    GenericFor {
        /// Loop variable names.
        names: Vec<Symbol>,
        /// Iterator expressions.
        iter: Vec<LuaExpr>,
        /// Loop body.
        block: LuaBlock,
    },
    /// A global or table-qualified function declaration.
    Function {
        /// Function name.
        name: LuaFunctionName,
        /// Function body.
        body: LuaFuncBody,
    },
    /// A `local function` declaration.
    LocalFunction {
        /// Function name.
        name: Symbol,
        /// Function body.
        body: LuaFuncBody,
    },
    /// A `return` statement.
    Return(Vec<LuaExpr>),
    /// A `break` statement.
    Break,
    /// A `::label::` declaration.
    Label(Symbol),
    /// A `goto` statement.
    Goto(Symbol),
    /// A `do ... end` block.
    Do(LuaBlock),
    /// A call or other expression statement.
    Expr(LuaExpr),
}

/// A local binding name and optional Lua 5.4 attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuaBinding {
    /// Binding name.
    pub name: Symbol,
    /// Optional `<const>` or `<close>` attribute.
    pub attr: Option<LuaLocalAttr>,
}

/// Lua 5.4 local variable attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LuaLocalAttr {
    /// The `<const>` local attribute.
    Const,
    /// The `<close>` local attribute.
    Close,
}

/// One `if` or `elseif` arm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuaIfArm {
    /// Arm condition.
    pub condition: LuaExpr,
    /// Arm body.
    pub block: LuaBlock,
}

/// A function declaration name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuaFunctionName {
    /// Base name.
    pub base: Symbol,
    /// Dot-qualified field path.
    pub fields: Vec<Symbol>,
    /// Optional method suffix from `:name`.
    pub method: Option<Symbol>,
}

/// Lua expression node used by the codec lexer/parser stage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LuaExpr {
    /// The `nil` literal.
    Nil,
    /// The `true` literal.
    True,
    /// The `false` literal.
    False,
    /// A number literal, preserving its source spelling.
    Number(String),
    /// A string literal after Lua escape or long-string decoding.
    Str(String),
    /// The vararg expression `...`.
    Vararg,
    /// A name reference.
    Name(Symbol),
    /// An indexed expression such as `t[k]` or `t.name`.
    Index {
        /// The receiver being indexed.
        obj: Box<LuaExpr>,
        /// The key expression.
        key: Box<LuaExpr>,
    },
    /// A function-style call.
    Call {
        /// The callee expression.
        callee: Box<LuaExpr>,
        /// Positional call arguments.
        args: Vec<LuaExpr>,
    },
    /// A method-style call such as `obj:name(args)`.
    Method {
        /// The receiver expression.
        recv: Box<LuaExpr>,
        /// The method name.
        name: Symbol,
        /// Positional call arguments.
        args: Vec<LuaExpr>,
    },
    /// A unary operator expression.
    Unary {
        /// The operator.
        op: LuaUnOp,
        /// The right-hand operand.
        rhs: Box<LuaExpr>,
    },
    /// A binary operator expression.
    Binary {
        /// The operator.
        op: LuaBinOp,
        /// The left-hand operand.
        lhs: Box<LuaExpr>,
        /// The right-hand operand.
        rhs: Box<LuaExpr>,
    },
    /// A table constructor.
    Table(Vec<LuaField>),
    /// A function literal.
    Function(LuaFuncBody),
}

/// Lua table-constructor field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LuaField {
    /// Positional field value.
    Positional(LuaExpr),
    /// Named field such as `name = value`.
    Named {
        /// The field name.
        key: Symbol,
        /// The field value.
        value: LuaExpr,
    },
    /// Keyed field such as `[expr] = value`.
    Keyed {
        /// The key expression.
        key: LuaExpr,
        /// The field value.
        value: LuaExpr,
    },
}

/// Lua function-literal body header.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LuaFuncBody {
    /// Named parameters.
    pub params: Vec<Symbol>,
    /// Whether the body accepts `...`.
    pub vararg: bool,
    /// Parsed function body block.
    pub block: LuaBlock,
}

/// Lua unary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LuaUnOp {
    /// Logical negation, `not`.
    Not,
    /// Length, `#`.
    Len,
    /// Arithmetic negation, `-`.
    Neg,
    /// Bitwise not, `~`.
    BitNot,
}

impl LuaUnOp {
    pub(crate) fn from_symbol(raw: &str) -> Option<Self> {
        match raw {
            "not" => Some(Self::Not),
            "#" => Some(Self::Len),
            "-" => Some(Self::Neg),
            "~" => Some(Self::BitNot),
            _ => None,
        }
    }
}

/// Lua binary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LuaBinOp {
    /// Logical `or`.
    Or,
    /// Logical `and`.
    And,
    /// Less-than comparison.
    Lt,
    /// Greater-than comparison.
    Gt,
    /// Less-than-or-equal comparison.
    Le,
    /// Greater-than-or-equal comparison.
    Ge,
    /// Not-equal comparison.
    Ne,
    /// Equal comparison.
    Eq,
    /// Bitwise or.
    BitOr,
    /// Bitwise exclusive or.
    BitXor,
    /// Bitwise and.
    BitAnd,
    /// Shift left.
    Shl,
    /// Shift right.
    Shr,
    /// String concatenation.
    Concat,
    /// Addition.
    Add,
    /// Subtraction.
    Sub,
    /// Multiplication.
    Mul,
    /// Division.
    Div,
    /// Floor division.
    FloorDiv,
    /// Modulo.
    Mod,
    /// Exponentiation.
    Pow,
}

impl LuaBinOp {
    pub(crate) fn from_symbol(raw: &str) -> Option<Self> {
        match raw {
            "or" => Some(Self::Or),
            "and" => Some(Self::And),
            "<" => Some(Self::Lt),
            ">" => Some(Self::Gt),
            "<=" => Some(Self::Le),
            ">=" => Some(Self::Ge),
            "~=" => Some(Self::Ne),
            "==" => Some(Self::Eq),
            "|" => Some(Self::BitOr),
            "~" => Some(Self::BitXor),
            "&" => Some(Self::BitAnd),
            "<<" => Some(Self::Shl),
            ">>" => Some(Self::Shr),
            ".." => Some(Self::Concat),
            "+" => Some(Self::Add),
            "-" => Some(Self::Sub),
            "*" => Some(Self::Mul),
            "/" => Some(Self::Div),
            "//" => Some(Self::FloorDiv),
            "%" => Some(Self::Mod),
            "^" => Some(Self::Pow),
            _ => None,
        }
    }
}
