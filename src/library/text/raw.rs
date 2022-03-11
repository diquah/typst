use once_cell::sync::Lazy;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Highlighter, Style, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

use crate::library::prelude::*;
use crate::library::text::TextNode;
use crate::source::SourceId;
use crate::syntax::{self, RedNode};

/// The lazily-loaded theme used for syntax highlighting.
static THEME: Lazy<Theme> =
    Lazy::new(|| ThemeSet::load_defaults().themes.remove("InspiredGitHub").unwrap());

/// The lazily-loaded syntect syntax definitions.
static SYNTAXES: Lazy<SyntaxSet> = Lazy::new(|| SyntaxSet::load_defaults_newlines());

/// Monospaced text with optional syntax highlighting.
#[derive(Debug, Hash)]
pub struct RawNode {
    /// The raw text.
    pub text: EcoString,
    /// Whether the node is block-level.
    pub block: bool,
}

#[class]
impl RawNode {
    /// The language to syntax-highlight in.
    pub const LANG: Option<EcoString> = None;

    fn construct(_: &mut Context, args: &mut Args) -> TypResult<Content> {
        Ok(Content::show(Self {
            text: args.expect("text")?,
            block: args.named("block")?.unwrap_or(false),
        }))
    }
}

impl Show for RawNode {
    fn show(&self, ctx: &mut Context, styles: StyleChain) -> TypResult<Content> {
        let lang = styles.get_ref(Self::LANG).as_ref();

        if let Some(content) = styles.show(self, ctx, [
            Value::Str(self.text.clone()),
            match lang {
                Some(lang) => Value::Str(lang.clone()),
                None => Value::None,
            },
            Value::Bool(self.block),
        ])? {
            return Ok(content);
        }

        let foreground = THEME
            .settings
            .foreground
            .map(Color::from)
            .unwrap_or(Color::BLACK)
            .into();

        let mut content = if matches!(
            lang.map(|s| s.to_lowercase()).as_deref(),
            Some("typ" | "typst")
        ) {
            let mut seq = vec![];
            let green = crate::parse::parse(&self.text);
            let red = RedNode::from_root(green, SourceId::from_raw(0));
            let highlighter = Highlighter::new(&THEME);

            syntax::highlight_syntect(red.as_ref(), &highlighter, &mut |range, style| {
                seq.push(styled(&self.text[range], foreground, style));
            });

            Content::sequence(seq)
        } else if let Some(syntax) =
            lang.and_then(|token| SYNTAXES.find_syntax_by_token(&token))
        {
            let mut seq = vec![];
            let mut highlighter = HighlightLines::new(syntax, &THEME);
            for (i, line) in self.text.lines().enumerate() {
                if i != 0 {
                    seq.push(Content::Linebreak);
                }

                for (style, piece) in highlighter.highlight(line, &SYNTAXES) {
                    seq.push(styled(piece, foreground, style));
                }
            }

            Content::sequence(seq)
        } else {
            Content::Text(self.text.clone())
        };

        if self.block {
            content = Content::Block(content.pack());
        }

        Ok(content.monospaced())
    }
}

/// Style a piece of text with a syntect style.
fn styled(piece: &str, foreground: Paint, style: Style) -> Content {
    let mut styles = StyleMap::new();
    let mut body = Content::Text(piece.into());

    let paint = style.foreground.into();
    if paint != foreground {
        styles.set(TextNode::FILL, paint);
    }

    if style.font_style.contains(FontStyle::BOLD) {
        styles.set(TextNode::STRONG, true);
    }

    if style.font_style.contains(FontStyle::ITALIC) {
        styles.set(TextNode::EMPH, true);
    }

    if style.font_style.contains(FontStyle::UNDERLINE) {
        body = body.underlined();
    }

    body.styled_with_map(styles)
}
