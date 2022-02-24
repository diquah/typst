//! Text shaping and styling.

use std::borrow::Cow;
use std::fmt::{self, Debug, Formatter};
use std::ops::{BitXor, Range};

use kurbo::{BezPath, Line, ParamCurve};
use rustybuzz::{Feature, UnicodeBuffer};
use ttf_parser::{GlyphId, OutlineBuilder, Tag};

use super::prelude::*;
use super::Decoration;
use crate::font::{
    Face, FaceId, FontStore, FontStretch, FontStyle, FontVariant, FontWeight,
    VerticalFontMetric,
};
use crate::geom::{Dir, Em, Length, Point, Size};
use crate::util::{EcoString, SliceExt};

/// A single run of text with the same style.
#[derive(Hash)]
pub struct TextNode;

#[class]
impl TextNode {
    /// A prioritized sequence of font families.
    #[variadic]
    pub const FAMILY: Vec<FontFamily> = vec![FontFamily::SansSerif];
    /// The serif font family/families.
    pub const SERIF: Vec<NamedFamily> = vec![NamedFamily::new("IBM Plex Serif")];
    /// The sans-serif font family/families.
    pub const SANS_SERIF: Vec<NamedFamily> = vec![NamedFamily::new("IBM Plex Sans")];
    /// The monospace font family/families.
    pub const MONOSPACE: Vec<NamedFamily> = vec![NamedFamily::new("IBM Plex Mono")];
    /// Whether to allow font fallback when the primary font list contains no
    /// match.
    pub const FALLBACK: bool = true;

    /// How the font is styled.
    pub const STYLE: FontStyle = FontStyle::Normal;
    /// The boldness / thickness of the font's glyphs.
    pub const WEIGHT: FontWeight = FontWeight::REGULAR;
    /// The width of the glyphs.
    pub const STRETCH: FontStretch = FontStretch::NORMAL;
    /// The glyph fill color.
    #[shorthand]
    pub const FILL: Paint = Color::BLACK.into();

    /// The size of the glyphs.
    #[shorthand]
    #[fold(Linear::compose)]
    pub const SIZE: Linear = Length::pt(11.0).into();
    /// The amount of space that should be added between characters.
    pub const TRACKING: Em = Em::zero();
    /// The top end of the text bounding box.
    pub const TOP_EDGE: VerticalFontMetric = VerticalFontMetric::CapHeight;
    /// The bottom end of the text bounding box.
    pub const BOTTOM_EDGE: VerticalFontMetric = VerticalFontMetric::Baseline;

    /// Whether to apply kerning ("kern").
    pub const KERNING: bool = true;
    /// Whether small capital glyphs should be used. ("smcp")
    pub const SMALLCAPS: bool = false;
    /// Whether to apply stylistic alternates. ("salt")
    pub const ALTERNATES: bool = false;
    /// Which stylistic set to apply. ("ss01" - "ss20")
    pub const STYLISTIC_SET: Option<StylisticSet> = None;
    /// Whether standard ligatures are active. ("liga", "clig")
    pub const LIGATURES: bool = true;
    /// Whether ligatures that should be used sparingly are active. ("dlig")
    pub const DISCRETIONARY_LIGATURES: bool = false;
    /// Whether historical ligatures are active. ("hlig")
    pub const HISTORICAL_LIGATURES: bool = false;
    /// Which kind of numbers / figures to select.
    pub const NUMBER_TYPE: Smart<NumberType> = Smart::Auto;
    /// The width of numbers / figures.
    pub const NUMBER_WIDTH: Smart<NumberWidth> = Smart::Auto;
    /// How to position numbers.
    pub const NUMBER_POSITION: NumberPosition = NumberPosition::Normal;
    /// Whether to have a slash through the zero glyph. ("zero")
    pub const SLASHED_ZERO: bool = false;
    /// Whether to convert fractions. ("frac")
    pub const FRACTIONS: bool = false;
    /// Raw OpenType features to apply.
    pub const FEATURES: Vec<(Tag, u32)> = vec![];

    /// Whether the font weight should be increased by 300.
    #[skip]
    #[fold(bool::bitxor)]
    pub const STRONG: bool = false;
    /// Whether the the font style should be inverted.
    #[skip]
    #[fold(bool::bitxor)]
    pub const EMPH: bool = false;
    /// Whether a monospace font should be preferred.
    #[skip]
    pub const MONOSPACED: bool = false;
    /// The case transformation that should be applied to the next.
    #[skip]
    pub const CASE: Option<Case> = None;
    /// Decorative lines.
    #[skip]
    #[fold(|a, b| a.into_iter().chain(b).collect())]
    pub const LINES: Vec<Decoration> = vec![];
    /// An URL the text should link to.
    #[skip]
    pub const LINK: Option<EcoString> = None;

    fn construct(_: &mut Context, args: &mut Args) -> TypResult<Template> {
        // The text constructor is special: It doesn't create a text node.
        // Instead, it leaves the passed argument structurally unchanged, but
        // styles all text in it.
        args.expect("body")
    }
}

/// Strong text, rendered in boldface.
#[derive(Debug, Hash)]
pub struct StrongNode(pub Template);

#[class]
impl StrongNode {
    fn construct(_: &mut Context, args: &mut Args) -> TypResult<Template> {
        Ok(Template::show(Self(args.expect("body")?)))
    }
}

impl Show for StrongNode {
    fn show(&self, ctx: &mut Context, styles: StyleChain) -> TypResult<Template> {
        Ok(styles
            .show(self, ctx, [Value::Template(self.0.clone())])?
            .unwrap_or_else(|| self.0.clone().styled(TextNode::STRONG, true)))
    }
}

/// Emphasized text, rendered with an italic face.
#[derive(Debug, Hash)]
pub struct EmphNode(pub Template);

#[class]
impl EmphNode {
    fn construct(_: &mut Context, args: &mut Args) -> TypResult<Template> {
        Ok(Template::show(Self(args.expect("body")?)))
    }
}

impl Show for EmphNode {
    fn show(&self, ctx: &mut Context, styles: StyleChain) -> TypResult<Template> {
        Ok(styles
            .show(self, ctx, [Value::Template(self.0.clone())])?
            .unwrap_or_else(|| self.0.clone().styled(TextNode::EMPH, true)))
    }
}

/// A generic or named font family.
#[derive(Clone, Eq, PartialEq, Hash)]
pub enum FontFamily {
    /// A family that has "serifs", small strokes attached to letters.
    Serif,
    /// A family in which glyphs do not have "serifs", small attached strokes.
    SansSerif,
    /// A family in which (almost) all glyphs are of equal width.
    Monospace,
    /// A specific font family like "Arial".
    Named(NamedFamily),
}

impl Debug for FontFamily {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Serif => f.pad("serif"),
            Self::SansSerif => f.pad("sans-serif"),
            Self::Monospace => f.pad("monospace"),
            Self::Named(s) => s.fmt(f),
        }
    }
}

dynamic! {
    FontFamily: "font family",
    Value::Str(string) => Self::Named(NamedFamily::new(&string)),
}

castable! {
    Vec<FontFamily>,
    Expected: "string, generic family or array thereof",
    Value::Str(string) => vec![FontFamily::Named(NamedFamily::new(&string))],
    Value::Array(values) => {
        values.into_iter().filter_map(|v| v.cast().ok()).collect()
    },
    @family: FontFamily => vec![family.clone()],
}

/// A specific font family like "Arial".
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct NamedFamily(EcoString);

impl NamedFamily {
    /// Create a named font family variant.
    pub fn new(string: &str) -> Self {
        Self(string.to_lowercase().into())
    }

    /// The lowercased family name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Debug for NamedFamily {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

castable! {
    Vec<NamedFamily>,
    Expected: "string or array of strings",
    Value::Str(string) => vec![NamedFamily::new(&string)],
    Value::Array(values) => values
        .into_iter()
        .filter_map(|v| v.cast().ok())
        .map(|string: EcoString| NamedFamily::new(&string))
        .collect(),
}

castable! {
    FontStyle,
    Expected: "string",
    Value::Str(string) => match string.as_str() {
        "normal" => Self::Normal,
        "italic" => Self::Italic,
        "oblique" => Self::Oblique,
        _ => Err(r#"expected "normal", "italic" or "oblique""#)?,
    },
}

castable! {
    FontWeight,
    Expected: "integer or string",
    Value::Int(v) => Value::Int(v)
        .cast::<usize>()?
        .try_into()
        .map_or(Self::BLACK, Self::from_number),
    Value::Str(string) => match string.as_str() {
        "thin" => Self::THIN,
        "extralight" => Self::EXTRALIGHT,
        "light" => Self::LIGHT,
        "regular" => Self::REGULAR,
        "medium" => Self::MEDIUM,
        "semibold" => Self::SEMIBOLD,
        "bold" => Self::BOLD,
        "extrabold" => Self::EXTRABOLD,
        "black" => Self::BLACK,
        _ => Err("unknown font weight")?,
    },
}

castable! {
    FontStretch,
    Expected: "relative",
    Value::Relative(v) => Self::from_ratio(v.get() as f32),
}

castable! {
    Em,
    Expected: "float",
    Value::Float(v) => Self::new(v),
}

castable! {
    VerticalFontMetric,
    Expected: "linear or string",
    Value::Length(v) => Self::Linear(v.into()),
    Value::Relative(v) => Self::Linear(v.into()),
    Value::Linear(v) => Self::Linear(v),
    Value::Str(string) => match string.as_str() {
        "ascender" => Self::Ascender,
        "cap-height" => Self::CapHeight,
        "x-height" => Self::XHeight,
        "baseline" => Self::Baseline,
        "descender" => Self::Descender,
        _ => Err("unknown font metric")?,
    },
}

/// A stylistic set in a font face.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct StylisticSet(u8);

impl StylisticSet {
    /// Creates a new set, clamping to 1-20.
    pub fn new(index: u8) -> Self {
        Self(index.clamp(1, 20))
    }

    /// Get the value, guaranteed to be 1-20.
    pub fn get(self) -> u8 {
        self.0
    }
}

castable! {
    StylisticSet,
    Expected: "integer",
    Value::Int(v) => match v {
        1 ..= 20 => Self::new(v as u8),
        _ => Err("must be between 1 and 20")?,
    },
}

/// Which kind of numbers / figures to select.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum NumberType {
    /// Numbers that fit well with capital text. ("lnum")
    Lining,
    /// Numbers that fit well into flow of upper- and lowercase text. ("onum")
    OldStyle,
}

castable! {
    NumberType,
    Expected: "string",
    Value::Str(string) => match string.as_str() {
        "lining" => Self::Lining,
        "old-style" => Self::OldStyle,
        _ => Err(r#"expected "lining" or "old-style""#)?,
    },
}

/// The width of numbers / figures.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum NumberWidth {
    /// Number widths are glyph specific. ("pnum")
    Proportional,
    /// All numbers are of equal width / monospaced. ("tnum")
    Tabular,
}

castable! {
    NumberWidth,
    Expected: "string",
    Value::Str(string) => match string.as_str() {
        "proportional" => Self::Proportional,
        "tabular" => Self::Tabular,
        _ => Err(r#"expected "proportional" or "tabular""#)?,
    },
}

/// How to position numbers.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum NumberPosition {
    /// Numbers are positioned on the same baseline as text.
    Normal,
    /// Numbers are smaller and placed at the bottom. ("subs")
    Subscript,
    /// Numbers are smaller and placed at the top. ("sups")
    Superscript,
}

castable! {
    NumberPosition,
    Expected: "string",
    Value::Str(string) => match string.as_str() {
        "normal" => Self::Normal,
        "subscript" => Self::Subscript,
        "superscript" => Self::Superscript,
        _ => Err(r#"expected "normal", "subscript" or "superscript""#)?,
    },
}

castable! {
    Vec<(Tag, u32)>,
    Expected: "array of strings or dictionary mapping tags to integers",
    Value::Array(values) => values
        .into_iter()
        .filter_map(|v| v.cast().ok())
        .map(|string: EcoString| (Tag::from_bytes_lossy(string.as_bytes()), 1))
        .collect(),
    Value::Dict(values) => values
        .into_iter()
        .filter_map(|(k, v)| {
            let tag = Tag::from_bytes_lossy(k.as_bytes());
            let num = v.cast::<i64>().ok()?.try_into().ok()?;
            Some((tag, num))
        })
        .collect(),
}

/// A case transformation on text.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum Case {
    /// Everything is uppercased.
    Upper,
    /// Everything is lowercased.
    Lower,
}

impl Case {
    /// Apply the case to a string of text.
    pub fn apply(self, text: &str) -> String {
        match self {
            Self::Upper => text.to_uppercase(),
            Self::Lower => text.to_lowercase(),
        }
    }
}

/// Shape text into [`ShapedText`].
pub fn shape<'a>(
    fonts: &mut FontStore,
    text: &'a str,
    styles: StyleChain<'a>,
    dir: Dir,
) -> ShapedText<'a> {
    let text = match styles.get(TextNode::CASE) {
        Some(case) => Cow::Owned(case.apply(text)),
        None => Cow::Borrowed(text),
    };

    let mut glyphs = vec![];
    if !text.is_empty() {
        shape_segment(
            fonts,
            &mut glyphs,
            0,
            &text,
            variant(styles),
            families(styles),
            None,
            dir,
            &tags(styles),
        );
    }

    track(&mut glyphs, styles.get(TextNode::TRACKING));
    let (size, baseline) = measure(fonts, &glyphs, styles);

    ShapedText {
        text,
        dir,
        styles,
        size,
        baseline,
        glyphs: Cow::Owned(glyphs),
    }
}

/// Shape text with font fallback using the `families` iterator.
fn shape_segment<'a>(
    fonts: &mut FontStore,
    glyphs: &mut Vec<ShapedGlyph>,
    base: usize,
    text: &str,
    variant: FontVariant,
    mut families: impl Iterator<Item = &'a str> + Clone,
    mut first_face: Option<FaceId>,
    dir: Dir,
    tags: &[rustybuzz::Feature],
) {
    // No font has newlines.
    if text.chars().all(|c| c == '\n') {
        return;
    }

    // Select the font family.
    let (face_id, fallback) = loop {
        // Try to load the next available font family.
        match families.next() {
            Some(family) => {
                if let Some(id) = fonts.select(family, variant) {
                    break (id, true);
                }
            }
            // We're out of families, so we don't do any more fallback and just
            // shape the tofus with the first face we originally used.
            None => match first_face {
                Some(id) => break (id, false),
                None => return,
            },
        }
    };

    // Remember the id if this the first available face since we use that one to
    // shape tofus.
    first_face.get_or_insert(face_id);

    // Fill the buffer with our text.
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_direction(match dir {
        Dir::LTR => rustybuzz::Direction::LeftToRight,
        Dir::RTL => rustybuzz::Direction::RightToLeft,
        _ => unimplemented!(),
    });

    // Shape!
    let mut face = fonts.get(face_id);
    let buffer = rustybuzz::shape(face.ttf(), tags, buffer);
    let infos = buffer.glyph_infos();
    let pos = buffer.glyph_positions();

    // Collect the shaped glyphs, doing fallback and shaping parts again with
    // the next font if necessary.
    let mut i = 0;
    while i < infos.len() {
        let info = &infos[i];
        let cluster = info.cluster as usize;

        if info.glyph_id != 0 || !fallback {
            // Add the glyph to the shaped output.
            // TODO: Don't ignore y_advance and y_offset.
            glyphs.push(ShapedGlyph {
                face_id,
                glyph_id: info.glyph_id as u16,
                x_advance: face.to_em(pos[i].x_advance),
                x_offset: face.to_em(pos[i].x_offset),
                text_index: base + cluster,
                safe_to_break: !info.unsafe_to_break(),
            });
        } else {
            // Determine the source text range for the tofu sequence.
            let range = {
                // First, search for the end of the tofu sequence.
                let k = i;
                while infos.get(i + 1).map_or(false, |info| info.glyph_id == 0) {
                    i += 1;
                }

                // Then, determine the start and end text index.
                //
                // Examples:
                // Everything is shown in visual order. Tofus are written as "_".
                // We want to find out that the tofus span the text `2..6`.
                // Note that the clusters are longer than 1 char.
                //
                // Left-to-right:
                // Text:     h a l i h a l l o
                // Glyphs:   A   _   _   C   E
                // Clusters: 0   2   4   6   8
                //              k=1 i=2
                //
                // Right-to-left:
                // Text:     O L L A H I L A H
                // Glyphs:   E   C   _   _   A
                // Clusters: 8   6   4   2   0
                //                  k=2 i=3
                let ltr = dir.is_positive();
                let first = if ltr { k } else { i };
                let start = infos[first].cluster as usize;
                let last = if ltr { i.checked_add(1) } else { k.checked_sub(1) };
                let end = last
                    .and_then(|last| infos.get(last))
                    .map_or(text.len(), |info| info.cluster as usize);

                start .. end
            };

            // Recursively shape the tofu sequence with the next family.
            shape_segment(
                fonts,
                glyphs,
                base + range.start,
                &text[range],
                variant,
                families.clone(),
                first_face,
                dir,
                tags,
            );

            face = fonts.get(face_id);
        }

        i += 1;
    }
}

/// Apply tracking to a slice of shaped glyphs.
fn track(glyphs: &mut [ShapedGlyph], tracking: Em) {
    if tracking.is_zero() {
        return;
    }

    let mut glyphs = glyphs.iter_mut().peekable();
    while let Some(glyph) = glyphs.next() {
        if glyphs
            .peek()
            .map_or(false, |next| glyph.text_index != next.text_index)
        {
            glyph.x_advance += tracking;
        }
    }
}

/// Measure the size and baseline of a run of shaped glyphs with the given
/// properties.
fn measure(
    fonts: &mut FontStore,
    glyphs: &[ShapedGlyph],
    styles: StyleChain,
) -> (Size, Length) {
    let mut width = Length::zero();
    let mut top = Length::zero();
    let mut bottom = Length::zero();

    let size = styles.get(TextNode::SIZE).abs;
    let top_edge = styles.get(TextNode::TOP_EDGE);
    let bottom_edge = styles.get(TextNode::BOTTOM_EDGE);

    // Expand top and bottom by reading the face's vertical metrics.
    let mut expand = |face: &Face| {
        top.set_max(face.vertical_metric(top_edge, size));
        bottom.set_max(-face.vertical_metric(bottom_edge, size));
    };

    if glyphs.is_empty() {
        // When there are no glyphs, we just use the vertical metrics of the
        // first available font.
        for family in families(styles) {
            if let Some(face_id) = fonts.select(family, variant(styles)) {
                expand(fonts.get(face_id));
                break;
            }
        }
    } else {
        for (face_id, group) in glyphs.group_by_key(|g| g.face_id) {
            let face = fonts.get(face_id);
            expand(face);

            for glyph in group {
                width += glyph.x_advance.resolve(size);
            }
        }
    }

    (Size::new(width, top + bottom), top)
}

/// Resolve the font variant with `STRONG` and `EMPH` factored in.
fn variant(styles: StyleChain) -> FontVariant {
    let mut variant = FontVariant::new(
        styles.get(TextNode::STYLE),
        styles.get(TextNode::WEIGHT),
        styles.get(TextNode::STRETCH),
    );

    if styles.get(TextNode::STRONG) {
        variant.weight = variant.weight.thicken(300);
    }

    if styles.get(TextNode::EMPH) {
        variant.style = match variant.style {
            FontStyle::Normal => FontStyle::Italic,
            FontStyle::Italic => FontStyle::Normal,
            FontStyle::Oblique => FontStyle::Normal,
        }
    }

    variant
}

/// Resolve a prioritized iterator over the font families.
fn families(styles: StyleChain) -> impl Iterator<Item = &str> + Clone {
    let head = if styles.get(TextNode::MONOSPACED) {
        styles.get_ref(TextNode::MONOSPACE).as_slice()
    } else {
        &[]
    };

    let core = styles.get_ref(TextNode::FAMILY).iter().flat_map(move |family| {
        match family {
            FontFamily::Named(name) => std::slice::from_ref(name),
            FontFamily::Serif => styles.get_ref(TextNode::SERIF),
            FontFamily::SansSerif => styles.get_ref(TextNode::SANS_SERIF),
            FontFamily::Monospace => styles.get_ref(TextNode::MONOSPACE),
        }
    });

    let tail: &[&str] = if styles.get(TextNode::FALLBACK) {
        &["ibm plex sans", "latin modern math", "twitter color emoji"]
    } else {
        &[]
    };

    head.iter()
        .chain(core)
        .map(|named| named.as_str())
        .chain(tail.iter().copied())
}

/// Collect the tags of the OpenType features to apply.
fn tags(styles: StyleChain) -> Vec<Feature> {
    let mut tags = vec![];
    let mut feat = |tag, value| {
        tags.push(Feature::new(Tag::from_bytes(tag), value, ..));
    };

    // Features that are on by default in Harfbuzz are only added if disabled.
    if !styles.get(TextNode::KERNING) {
        feat(b"kern", 0);
    }

    // Features that are off by default in Harfbuzz are only added if enabled.
    if styles.get(TextNode::SMALLCAPS) {
        feat(b"smcp", 1);
    }

    if styles.get(TextNode::ALTERNATES) {
        feat(b"salt", 1);
    }

    let storage;
    if let Some(set) = styles.get(TextNode::STYLISTIC_SET) {
        storage = [b's', b's', b'0' + set.get() / 10, b'0' + set.get() % 10];
        feat(&storage, 1);
    }

    if !styles.get(TextNode::LIGATURES) {
        feat(b"liga", 0);
        feat(b"clig", 0);
    }

    if styles.get(TextNode::DISCRETIONARY_LIGATURES) {
        feat(b"dlig", 1);
    }

    if styles.get(TextNode::HISTORICAL_LIGATURES) {
        feat(b"hilg", 1);
    }

    match styles.get(TextNode::NUMBER_TYPE) {
        Smart::Auto => {}
        Smart::Custom(NumberType::Lining) => feat(b"lnum", 1),
        Smart::Custom(NumberType::OldStyle) => feat(b"onum", 1),
    }

    match styles.get(TextNode::NUMBER_WIDTH) {
        Smart::Auto => {}
        Smart::Custom(NumberWidth::Proportional) => feat(b"pnum", 1),
        Smart::Custom(NumberWidth::Tabular) => feat(b"tnum", 1),
    }

    match styles.get(TextNode::NUMBER_POSITION) {
        NumberPosition::Normal => {}
        NumberPosition::Subscript => feat(b"subs", 1),
        NumberPosition::Superscript => feat(b"sups", 1),
    }

    if styles.get(TextNode::SLASHED_ZERO) {
        feat(b"zero", 1);
    }

    if styles.get(TextNode::FRACTIONS) {
        feat(b"frac", 1);
    }

    for &(tag, value) in styles.get_ref(TextNode::FEATURES).iter() {
        tags.push(Feature::new(tag, value, ..))
    }

    tags
}

/// The result of shaping text.
///
/// This type contains owned or borrowed shaped text runs, which can be
/// measured, used to reshape substrings more quickly and converted into a
/// frame.
#[derive(Debug, Clone)]
pub struct ShapedText<'a> {
    /// The text that was shaped.
    pub text: Cow<'a, str>,
    /// The text direction.
    pub dir: Dir,
    /// The text's style properties.
    pub styles: StyleChain<'a>,
    /// The font size.
    pub size: Size,
    /// The baseline from the top of the frame.
    pub baseline: Length,
    /// The shaped glyphs.
    pub glyphs: Cow<'a, [ShapedGlyph]>,
}

/// A single glyph resulting from shaping.
#[derive(Debug, Copy, Clone)]
pub struct ShapedGlyph {
    /// The font face the glyph is contained in.
    pub face_id: FaceId,
    /// The glyph's index in the face.
    pub glyph_id: u16,
    /// The advance width of the glyph.
    pub x_advance: Em,
    /// The horizontal offset of the glyph.
    pub x_offset: Em,
    /// The start index of the glyph in the source text.
    pub text_index: usize,
    /// Whether splitting the shaping result before this glyph would yield the
    /// same results as shaping the parts to both sides of `text_index`
    /// separately.
    pub safe_to_break: bool,
}

impl<'a> ShapedText<'a> {
    /// Build the shaped text's frame.
    pub fn build(&self, fonts: &FontStore) -> Frame {
        let mut offset = Length::zero();
        let mut frame = Frame::new(self.size);
        frame.baseline = Some(self.baseline);

        for (face_id, group) in self.glyphs.as_ref().group_by_key(|g| g.face_id) {
            let pos = Point::new(offset, self.baseline);

            let size = self.styles.get(TextNode::SIZE).abs;
            let fill = self.styles.get(TextNode::FILL);
            let glyphs = group
                .iter()
                .map(|glyph| Glyph {
                    id: glyph.glyph_id,
                    x_advance: glyph.x_advance,
                    x_offset: glyph.x_offset,
                })
                .collect();

            let text = Text { face_id, size, fill, glyphs };
            let text_layer = frame.layer();
            let width = text.width();

            // Apply line decorations.
            for deco in self.styles.get_cloned(TextNode::LINES) {
                self.decorate(&mut frame, &deco, fonts, &text, pos, width);
            }

            frame.insert(text_layer, pos, Element::Text(text));
            offset += width;
        }

        // Apply link if it exists.
        if let Some(url) = self.styles.get_ref(TextNode::LINK) {
            frame.link(url);
        }

        frame
    }

    /// Add line decorations to a run of shaped text of a single font.
    fn decorate(
        &self,
        frame: &mut Frame,
        deco: &Decoration,
        fonts: &FontStore,
        text: &Text,
        pos: Point,
        width: Length,
    ) {
        let face = fonts.get(text.face_id);
        let metrics = match deco.line {
            super::STRIKETHROUGH => face.strikethrough,
            super::OVERLINE => face.overline,
            super::UNDERLINE | _ => face.underline,
        };

        let evade = deco.evade && deco.line != super::STRIKETHROUGH;
        let extent = deco.extent.resolve(text.size);
        let offset = deco
            .offset
            .map(|s| s.resolve(text.size))
            .unwrap_or(-metrics.position.resolve(text.size));

        let stroke = Stroke {
            paint: deco.stroke.unwrap_or(text.fill),
            thickness: deco
                .thickness
                .map(|s| s.resolve(text.size))
                .unwrap_or(metrics.thickness.resolve(text.size)),
        };

        let gap_padding = 0.08 * text.size;
        let min_width = 0.162 * text.size;

        let mut start = pos.x - extent;
        let end = pos.x + (width + 2.0 * extent);

        let mut push_segment = |from: Length, to: Length| {
            let origin = Point::new(from, pos.y + offset);
            let target = Point::new(to - from, Length::zero());

            if target.x >= min_width || !evade {
                let shape = Shape::stroked(Geometry::Line(target), stroke);
                frame.push(origin, Element::Shape(shape));
            }
        };

        if !evade {
            push_segment(start, end);
            return;
        }

        let line = Line::new(
            kurbo::Point::new(pos.x.to_raw(), offset.to_raw()),
            kurbo::Point::new((pos.x + width).to_raw(), offset.to_raw()),
        );

        let mut x = pos.x;
        let mut intersections = vec![];

        for glyph in text.glyphs.iter() {
            let dx = glyph.x_offset.resolve(text.size) + x;
            let mut builder =
                KurboPathBuilder::new(face.units_per_em, text.size, dx.to_raw());

            let bbox = face.ttf().outline_glyph(GlyphId(glyph.id), &mut builder);
            let path = builder.finish();

            x += glyph.x_advance.resolve(text.size);

            // Only do the costly segments intersection test if the line
            // intersects the bounding box.
            if bbox.map_or(false, |bbox| {
                let y_min = -face.to_em(bbox.y_max).resolve(text.size);
                let y_max = -face.to_em(bbox.y_min).resolve(text.size);

                offset >= y_min && offset <= y_max
            }) {
                // Find all intersections of segments with the line.
                intersections.extend(
                    path.segments()
                        .flat_map(|seg| seg.intersect_line(line))
                        .map(|is| Length::raw(line.eval(is.line_t).x)),
                );
            }
        }

        // When emitting the decorative line segments, we move from left to
        // right. The intersections are not necessarily in this order, yet.
        intersections.sort();

        for gap in intersections.chunks_exact(2) {
            let l = gap[0] - gap_padding;
            let r = gap[1] + gap_padding;

            if start >= end {
                break;
            }

            if start >= l {
                start = r;
                continue;
            }

            push_segment(start, l);
            start = r;
        }

        if start < end {
            push_segment(start, end);
        }
    }

    /// Reshape a range of the shaped text, reusing information from this
    /// shaping process if possible.
    pub fn reshape(
        &'a self,
        fonts: &mut FontStore,
        text_range: Range<usize>,
    ) -> ShapedText<'a> {
        if let Some(glyphs) = self.slice_safe_to_break(text_range.clone()) {
            let (size, baseline) = measure(fonts, glyphs, self.styles);
            Self {
                text: Cow::Borrowed(&self.text[text_range]),
                dir: self.dir,
                styles: self.styles.clone(),
                size,
                baseline,
                glyphs: Cow::Borrowed(glyphs),
            }
        } else {
            shape(fonts, &self.text[text_range], self.styles.clone(), self.dir)
        }
    }

    /// Find the subslice of glyphs that represent the given text range if both
    /// sides are safe to break.
    fn slice_safe_to_break(&self, text_range: Range<usize>) -> Option<&[ShapedGlyph]> {
        let Range { mut start, mut end } = text_range;
        if !self.dir.is_positive() {
            std::mem::swap(&mut start, &mut end);
        }

        let left = self.find_safe_to_break(start, Side::Left)?;
        let right = self.find_safe_to_break(end, Side::Right)?;
        Some(&self.glyphs[left .. right])
    }

    /// Find the glyph offset matching the text index that is most towards the
    /// given side and safe-to-break.
    fn find_safe_to_break(&self, text_index: usize, towards: Side) -> Option<usize> {
        let ltr = self.dir.is_positive();

        // Handle edge cases.
        let len = self.glyphs.len();
        if text_index == 0 {
            return Some(if ltr { 0 } else { len });
        } else if text_index == self.text.len() {
            return Some(if ltr { len } else { 0 });
        }

        // Find any glyph with the text index.
        let mut idx = self
            .glyphs
            .binary_search_by(|g| {
                let ordering = g.text_index.cmp(&text_index);
                if ltr { ordering } else { ordering.reverse() }
            })
            .ok()?;

        let next = match towards {
            Side::Left => usize::checked_sub,
            Side::Right => usize::checked_add,
        };

        // Search for the outermost glyph with the text index.
        while let Some(next) = next(idx, 1) {
            if self.glyphs.get(next).map_or(true, |g| g.text_index != text_index) {
                break;
            }
            idx = next;
        }

        // RTL needs offset one because the left side of the range should be
        // exclusive and the right side inclusive, contrary to the normal
        // behaviour of ranges.
        if !ltr {
            idx += 1;
        }

        self.glyphs[idx].safe_to_break.then(|| idx)
    }
}

/// A visual side.
enum Side {
    Left,
    Right,
}

struct KurboPathBuilder {
    path: BezPath,
    units_per_em: f64,
    font_size: Length,
    x_offset: f64,
}

impl KurboPathBuilder {
    fn new(units_per_em: f64, font_size: Length, x_offset: f64) -> Self {
        Self {
            path: BezPath::new(),
            units_per_em,
            font_size,
            x_offset,
        }
    }

    fn finish(self) -> BezPath {
        self.path
    }

    fn p(&self, x: f32, y: f32) -> kurbo::Point {
        kurbo::Point::new(self.s(x) + self.x_offset, -self.s(y))
    }

    fn s(&self, v: f32) -> f64 {
        Em::from_units(v, self.units_per_em).resolve(self.font_size).to_raw()
    }
}

impl OutlineBuilder for KurboPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(self.p(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(self.p(x, y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.path.quad_to(self.p(x1, y1), self.p(x, y));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.path.curve_to(self.p(x1, y1), self.p(x2, y2), self.p(x, y));
    }

    fn close(&mut self) {
        self.path.close_path();
    }
}
