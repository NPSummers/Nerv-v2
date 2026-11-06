use nerv_lexer::{Lexer, Token, TokenKind as K};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
};

pub(crate) fn files(paths: &[PathBuf], check: bool) -> Result<u8, String> {
    let mut changed = 0;
    let mut formatted = Vec::new();
    for path in paths {
        let source = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let output = format(&source).map_err(|e| format!("{}: {e}", path.display()))?;
        if output != source {
            formatted.push((path, output));
        }
    }
    for (path, output) in formatted {
        changed += 1;
        if check {
            eprintln!("Would format {}", path.display());
        } else {
            super::project::write_atomic(path, output.as_bytes())?;
            println!("Formatted {}", path.display());
        }
    }
    Ok(u8::from(check && changed > 0))
}

fn tokens(source: &str) -> Result<Vec<Token>, String> {
    Lexer::new(source)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn signature(source: &str) -> Result<Vec<(K, String)>, String> {
    Ok(tokens(source)?
        .into_iter()
        .map(|t| (t.kind, t.text(source).to_owned()))
        .map(|(k, text)| {
            if matches!(k, K::Indent | K::Dedent | K::Newline | K::Eof) {
                (k, String::new())
            } else {
                (k, text)
            }
        })
        .collect())
}

pub(crate) fn format(source: &str) -> Result<String, String> {
    let module = nerv_syntax::parse(source).map_err(|e| e.to_string())?;
    let tokens = tokens(source)?;
    let mut compact = BTreeSet::new();
    for usage in &module.type_uses {
        for (index, token) in tokens.iter().enumerate() {
            if token.span.start >= usage.span.start && token.span.end <= usage.span.end {
                compact.insert(index);
            }
        }
    }
    for index in 2..tokens.len() {
        if tokens[index].kind == K::Lt
            && matches!(tokens[index - 2].kind, K::Fn | K::Struct | K::Enum)
        {
            let mut depth = 0;
            for (j, token) in tokens.iter().enumerate().skip(index) {
                compact.insert(j);
                if token.kind == K::Lt {
                    depth += 1;
                }
                if token.kind == K::Gt {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
        }
    }
    let mut lines = BTreeMap::<usize, (usize, Vec<(usize, &Token)>)>::new();
    let mut depth = 0usize;
    let mut delimiters = 0usize;
    let mut continuation = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            K::Indent => {
                depth += 1;
                continue;
            }
            K::Dedent => {
                depth = depth.saturating_sub(1);
                continue;
            }
            K::Newline | K::Eof => continue,
            _ => {}
        }
        let indent = if delimiters == 0 {
            depth
        } else {
            continuation + usize::from(!matches!(token.kind, K::RParen | K::RBracket | K::RBrace))
        };
        lines
            .entry(token.line)
            .or_insert_with(|| (indent, Vec::new()))
            .1
            .push((index, token));
        if matches!(token.kind, K::LParen | K::LBracket | K::LBrace) {
            if delimiters == 0 {
                continuation = depth;
            }
            delimiters += 1;
        }
        if matches!(token.kind, K::RParen | K::RBracket | K::RBrace) {
            delimiters = delimiters.saturating_sub(1);
        }
    }
    let mut indentation = BTreeMap::new();
    for (line_number, line) in source.lines().enumerate() {
        if let Some((depth, _)) = lines.get(&(line_number + 1)) {
            let width = line
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .map(|c| if c == '\t' { 4 } else { 1 })
                .sum::<usize>();
            indentation.insert(width, *depth);
        }
    }
    let mut output = String::new();
    for (number, original) in source.lines().enumerate() {
        let original = original.trim_end();
        if original.trim().is_empty() {
            output.push('\n');
            continue;
        }
        if let Some((depth, tokens)) = lines.get(&(number + 1)) {
            output.push_str(&"    ".repeat(*depth));
            for (position, (index, token)) in tokens.iter().enumerate() {
                if position > 0 {
                    let (previous_index, previous) = tokens[position - 1];
                    let next = tokens.get(position + 1).map(|(_, t)| t.kind);
                    let unary = matches!(previous.kind, K::Minus | K::Plus)
                        && (position < 2
                            || matches!(
                                tokens[position - 2].1.kind,
                                K::LParen
                                    | K::LBracket
                                    | K::Comma
                                    | K::Eq
                                    | K::FatArrow
                                    | K::Return
                                    | K::Plus
                                    | K::Minus
                                    | K::Star
                                    | K::Slash
                            ));
                    let generic = compact.contains(index) && compact.contains(&previous_index);
                    let method = previous.kind == K::Colon && next == Some(K::LParen);
                    let no_space = matches!(
                        token.kind,
                        K::Comma | K::Semicolon | K::RParen | K::RBracket | K::Dot | K::Colon
                    ) || matches!(previous.kind, K::LParen | K::LBracket | K::Dot)
                        || (token.kind == K::LParen
                            && matches!(
                                previous.kind,
                                K::Ident
                                    | K::Some
                                    | K::None
                                    | K::Fn
                                    | K::RParen
                                    | K::RBracket
                                    | K::Gt
                            ))
                        || (generic
                            && (matches!(token.kind, K::Lt | K::Gt) || previous.kind == K::Lt))
                        || (generic && token.kind == K::Ident && previous.kind == K::RBracket)
                        || (previous.kind == K::LBrace && token.kind == K::RBrace)
                        || (unary && token.kind != K::Minus)
                        || method;
                    if !no_space {
                        output.push(' ');
                    }
                }
                output.push_str(token.text(source));
            }
            let last = tokens.last().unwrap().1;
            let line_start = source[..last.span.start]
                .rfind('\n')
                .map(|p| p + 1)
                .unwrap_or(0);
            let remainder = &original[last.span.end - line_start..];
            if !remainder.trim().is_empty() {
                output.push_str("  ");
                output.push_str(remainder.trim());
            }
        } else {
            let width = original
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .map(|c| if c == '\t' { 4 } else { 1 })
                .sum::<usize>();
            let depth = indentation.get(&width).copied().unwrap_or(width / 4);
            output.push_str(&"    ".repeat(depth));
            output.push_str(original.trim_start());
        }
        output.push('\n');
    }
    while output.ends_with("\n\n") {
        output.pop();
    }
    if signature(source)? != signature(&output)? {
        return Err("formatting would alter the token structure; source was left unchanged".into());
    }
    nerv_syntax::parse(&output).map_err(|e| format!("formatting would alter syntax: {e}"))?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_layout_and_keeps_comments_and_strings() {
        let source = "-- heading\nfn main( )->i64\n  let value= -1 -- value\n  println(\"a--b\")\n  value+2\n";
        let expected = "-- heading\nfn main() -> i64\n    let value = -1  -- value\n    println(\"a--b\")\n    value + 2\n";
        assert_eq!(format(source).unwrap(), expected);
        assert_eq!(format(expected).unwrap(), expected);
    }

    #[test]
    fn formats_repository_examples_without_altering_tokens() {
        for source in [
            include_str!("../../../example/generics.nerv"),
            include_str!("../../../stdlib/std.nerv"),
            "fn main() -> i64\n    let x = [\n        1,\n        2\n    ]\n    x[0]\n",
        ] {
            let output = format(source).unwrap();
            assert_eq!(format(&output).unwrap(), output);
        }
    }
}
