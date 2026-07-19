use sim_codec::DecodeBudget;
use sim_codec_pratt::{PrattCodecParser, PrattTokenSource, SpannedPrattToken};
use sim_kernel::{
    CodecId, Error, Fixity, PrattOperator, PrattResult, PrattTable, PrattToken, Result, Symbol,
};

use crate::lex::{LuaTokenKind, tokenize_lua_with_budget};

/// Token source that adapts Lua lexical tokens to the shared Pratt driver.
#[derive(Clone, Copy, Debug, Default)]
pub struct LuaTokenSource;

impl PrattTokenSource for LuaTokenSource {
    fn tokenize_pratt(
        &self,
        codec: CodecId,
        source: &str,
        budget: &mut DecodeBudget,
    ) -> Result<Vec<SpannedPrattToken>> {
        tokenize_lua_with_budget(codec, source, budget)?
            .into_iter()
            .map(|token| {
                let pratt = token.kind.into_pratt_token().ok_or_else(|| {
                    Error::Eval(
                        "lua token is not part of the shared Pratt expression subset".to_owned(),
                    )
                })?;
                Ok(SpannedPrattToken::with_leading_trivia(
                    pratt,
                    token.start,
                    token.end,
                    token.leading_trivia,
                ))
            })
            .collect()
    }
}

/// Builds a shared Pratt parser using the Lua operator table and token source.
pub fn lua_pratt_parser() -> PrattCodecParser<LuaTokenSource> {
    PrattCodecParser::new(lua_pratt_table(), LuaTokenSource).with_surface_name("lua")
}

/// Builds the Lua 5.4 expression operator table.
pub fn lua_pratt_table() -> PrattTable {
    let mut table = PrattTable::new();
    infix(&mut table, "or", Fixity::InfixLeft, 10, 11);
    infix(&mut table, "and", Fixity::InfixLeft, 20, 21);
    for symbol in ["<", ">", "<=", ">=", "~=", "=="] {
        infix(&mut table, symbol, Fixity::InfixLeft, 30, 31);
    }
    infix(&mut table, "|", Fixity::InfixLeft, 40, 41);
    infix(&mut table, "~", Fixity::InfixLeft, 50, 51);
    infix(&mut table, "&", Fixity::InfixLeft, 60, 61);
    for symbol in ["<<", ">>"] {
        infix(&mut table, symbol, Fixity::InfixLeft, 70, 71);
    }
    infix(&mut table, "..", Fixity::InfixRight, 81, 80);
    for symbol in ["+", "-"] {
        infix(&mut table, symbol, Fixity::InfixLeft, 90, 91);
    }
    for symbol in ["*", "/", "//", "%"] {
        infix(&mut table, symbol, Fixity::InfixLeft, 100, 101);
    }
    for symbol in ["not", "#", "-", "~"] {
        prefix(&mut table, symbol, 121);
    }
    infix(&mut table, "^", Fixity::InfixRight, 141, 140);
    table
}

fn infix(table: &mut PrattTable, raw: &str, fixity: Fixity, left_bp: u16, right_bp: u16) {
    table.register(PrattOperator {
        symbol: Symbol::new(raw),
        fixity,
        left_bp,
        right_bp,
        result: PrattResult::ExprInfix,
    });
}

fn prefix(table: &mut PrattTable, raw: &str, right_bp: u16) {
    table.register(PrattOperator {
        symbol: Symbol::new(raw),
        fixity: Fixity::Prefix,
        left_bp: 0,
        right_bp,
        result: PrattResult::ExprPrefix,
    });
}

impl LuaTokenKind {
    pub(crate) fn into_pratt_token(self) -> Option<PrattToken> {
        match self {
            Self::Identifier(text) => Some(PrattToken::Ident(text)),
            Self::Number(text) => Some(PrattToken::Number(text)),
            Self::String(text) => Some(PrattToken::String(text)),
            Self::Operator(text) => Some(PrattToken::Operator(text)),
            Self::OpenParen => Some(PrattToken::OpenParen),
            Self::CloseParen => Some(PrattToken::CloseParen),
            Self::Comma => Some(PrattToken::Comma),
            _ => None,
        }
    }

    pub(crate) fn pratt_lookup_token(&self) -> Option<PrattToken> {
        match self {
            Self::Identifier(text) if matches!(text.as_str(), "and" | "or" | "not") => {
                Some(PrattToken::Ident(text.clone()))
            }
            Self::Operator(text) => Some(PrattToken::Operator(text.clone())),
            _ => None,
        }
    }
}
