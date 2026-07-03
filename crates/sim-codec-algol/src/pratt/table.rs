//! Operator table for the Algol codec: builds the default `PrattTable` of infix
//! and other operators with their fixities and binding powers, and reports
//! whether Pratt parsing is supported.

use sim_kernel::{Fixity, PrattOperator, PrattResult, PrattTable, Symbol};

/// Reports whether this codec offers Pratt parsing. Always `true` for the Algol
/// surface.
pub fn supports_pratt() -> bool {
    true
}

/// Builds the default Algol operator table.
///
/// Registers the arithmetic operators with their fixities and binding powers:
/// left-associative `+` and `-` (binding power 50/51), `*` and `/` (60/61),
/// right-associative `^` (80), prefix negation `-` (90), and postfix `!` (100).
/// Higher binding power binds more tightly, so `1 + 2 * 3` groups as
/// `1 + (2 * 3)`.
pub fn default_pratt_table() -> PrattTable {
    let mut table = PrattTable::new();
    table.register(PrattOperator {
        symbol: Symbol::new("+"),
        fixity: Fixity::InfixLeft,
        left_bp: 50,
        right_bp: 51,
        result: PrattResult::ExprInfix,
    });
    table.register(PrattOperator {
        symbol: Symbol::new("-"),
        fixity: Fixity::InfixLeft,
        left_bp: 50,
        right_bp: 51,
        result: PrattResult::ExprInfix,
    });
    table.register(PrattOperator {
        symbol: Symbol::new("*"),
        fixity: Fixity::InfixLeft,
        left_bp: 60,
        right_bp: 61,
        result: PrattResult::ExprInfix,
    });
    table.register(PrattOperator {
        symbol: Symbol::new("/"),
        fixity: Fixity::InfixLeft,
        left_bp: 60,
        right_bp: 61,
        result: PrattResult::ExprInfix,
    });
    table.register(PrattOperator {
        symbol: Symbol::new("^"),
        fixity: Fixity::InfixRight,
        left_bp: 80,
        right_bp: 80,
        result: PrattResult::ExprInfix,
    });
    table.register(PrattOperator {
        symbol: Symbol::new("-"),
        fixity: Fixity::Prefix,
        left_bp: 0,
        right_bp: 90,
        result: PrattResult::ExprPrefix,
    });
    table.register(PrattOperator {
        symbol: Symbol::new("!"),
        fixity: Fixity::Postfix,
        left_bp: 100,
        right_bp: 0,
        result: PrattResult::ExprPostfix,
    });
    table
}
