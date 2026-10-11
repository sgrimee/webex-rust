//! Style types for Adaptive Cards including colors, spacing, weights, and alignment options.

use serde::{Deserialize, Serialize};
use strum_macros::EnumString;

/// Color for text
#[allow(missing_docs)]
#[derive(Clone, Debug, Deserialize, EnumString, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
#[strum(ascii_case_insensitive)]
pub enum Color {
    Default,
    Dark,
    Light,
    Accent,
    Good,
    Warning,
    Attention,
}

/// Style hint for Container.
#[allow(missing_docs)]
#[derive(Clone, Debug, Deserialize, EnumString, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[strum(ascii_case_insensitive)]
pub enum ContainerStyle {
    Default,
    Emphasis,
    Good,
    Attention,
    Warning,
    Accent,
}

/// Controls the amount of spacing between this element and the preceding element.
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Deserialize, EnumString, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
#[strum(ascii_case_insensitive)]
pub enum Spacing {
    Default,
    None,
    Small,
    Medium,
    Large,
    ExtraLarge,
    Padding,
}

/// Style for Input.ChoiceSet
#[allow(missing_docs)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChoiceInputStyle {
    Compact,
    Expanded,
}

/// Vertical content alignment
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum VerticalContentAlignment {
    Top,
    Center,
    Bottom,
}

/// Text input style
#[allow(missing_docs)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TextInputStyle {
    Text,
    Tel,
    Url,
    Email,
}

/// Height of element
#[allow(missing_docs)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Height {
    Auto,
    Stretch,
}

/// Style hint for Image.
#[allow(missing_docs)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ImageStyle {
    Default,
    Person,
}

/// Weight of text
#[allow(missing_docs)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum Weight {
    Default,
    Lighter,
    Bolder,
}

/// Font type
#[allow(missing_docs)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum FontType {
    Default,
    Monospace,
}

/// Size of text
#[allow(missing_docs)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum Size {
    Default,
    Small,
    Medium,
    Large,
    ExtraLarge,
}

/// Horizontal alignment
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum HorizontalAlignment {
    Left,
    Center,
    Right,
}

/// Size of image (pixel width)
#[allow(missing_docs)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum ImageSize {
    Auto,
    Stretch,
    Small,
    Medium,
    Large,
}

#[cfg(test)]
mod tests {
    use super::{Color, ContainerStyle, Spacing};
    use std::str::FromStr;

    #[test]
    fn parses_adaptive_card_styles_case_insensitively() {
        assert_eq!(Color::from_str("aCcEnT"), Ok(Color::Accent));
        assert_eq!(
            ContainerStyle::from_str("emPHASIS"),
            Ok(ContainerStyle::Emphasis)
        );
        assert_eq!(Spacing::from_str("eXTRaLARge"), Ok(Spacing::ExtraLarge));
    }
}
