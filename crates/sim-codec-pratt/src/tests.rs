use sim_codec::{DecodeBudget, DecodeLimits};
use sim_kernel::{
    CodecId, Error, Expr, Fixity, PrattOperator, PrattResult, PrattTable, PrattToken as Token,
    Result, Symbol, Trivia,
};

use crate::{PrattCodecParser, PrattTokenSource, SpannedPrattToken};

// conformance: Pratt parser handles operator precedence and associativity.

#[derive(Clone)]
struct StaticTokens(Vec<SpannedPrattToken>);

impl PrattTokenSource for StaticTokens {
    fn tokenize_pratt(
        &self,
        _codec: CodecId,
        _source: &str,
        _budget: &mut DecodeBudget,
    ) -> Result<Vec<SpannedPrattToken>> {
        Ok(self.0.clone())
    }
}

#[test]
fn parses_token_source_with_operator_precedence() {
    let parser = PrattCodecParser::new(arithmetic_table(), StaticTokens(tokens_for_expr()));
    let tree = parser
        .parse_tree_with_source(CodecId(7), "calc.pratt", "1 + 2 * 3")
        .unwrap();

    let Expr::Infix {
        operator, right, ..
    } = &tree.expr
    else {
        panic!("expected top-level infix");
    };
    assert_eq!(operator, &Symbol::new("+"));
    assert!(matches!(
        right.as_ref(),
        Expr::Infix { operator, .. } if operator == &Symbol::new("*")
    ));
    assert_eq!(tree.origin.as_ref().unwrap().span.start, 0);
    assert_eq!(tree.origin.as_ref().unwrap().span.end, 9);
    assert_eq!(tree.children[1].origin.as_ref().unwrap().span.start, 4);
}

#[test]
fn carries_token_trivia_into_tree_origins() {
    let mut tokens = tokens_for_sum();
    tokens[2].leading_trivia = vec![Trivia::BlockComment("/* note */".to_owned())];
    let parser = PrattCodecParser::new(arithmetic_table(), StaticTokens(tokens));
    let tree = parser
        .parse_tree_with_source(CodecId(7), "calc.pratt", "1 + /* note */ 2")
        .unwrap();

    assert!(
        tree.origin
            .as_ref()
            .unwrap()
            .trivia
            .iter()
            .any(|trivia| matches!(trivia, Trivia::BlockComment(text) if text.contains("note")))
    );
}

#[test]
fn enforces_token_budget_after_token_source_runs() {
    let parser = PrattCodecParser::new(arithmetic_table(), StaticTokens(tokens_for_expr()));
    let mut budget = DecodeBudget::new(DecodeLimits {
        max_tokens: 2,
        ..DecodeLimits::default()
    });
    let err = parser
        .parse_tree_with_budget(CodecId(7), "1 + 2 * 3", &mut budget)
        .unwrap_err();

    assert!(matches!(err, Error::CodecError { message, .. } if message.contains("tokens")));
}

fn tokens_for_sum() -> Vec<SpannedPrattToken> {
    vec![
        SpannedPrattToken::new(Token::Number("1".to_owned()), 0, 1),
        SpannedPrattToken::new(Token::Operator("+".to_owned()), 2, 3),
        SpannedPrattToken::new(Token::Number("2".to_owned()), 4, 5),
    ]
}

fn tokens_for_expr() -> Vec<SpannedPrattToken> {
    vec![
        SpannedPrattToken::new(Token::Number("1".to_owned()), 0, 1),
        SpannedPrattToken::new(Token::Operator("+".to_owned()), 2, 3),
        SpannedPrattToken::new(Token::Number("2".to_owned()), 4, 5),
        SpannedPrattToken::new(Token::Operator("*".to_owned()), 6, 7),
        SpannedPrattToken::new(Token::Number("3".to_owned()), 8, 9),
    ]
}

fn arithmetic_table() -> PrattTable {
    let mut table = PrattTable::new();
    table.register(PrattOperator {
        symbol: Symbol::new("+"),
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
    table
}
