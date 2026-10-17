//! Macros of the parameter structs.

/// An enum of allowed text values. It reads the text without regard to the case and takes `-` for
/// `_` (`Hidden-Line` is `hidden_line`), as the methods always did. A variant can have other names
/// (`Shp = "shp" | "shapefile"`), the ones that the `from_name` functions of the program accept;
/// the schema lists only the first. A leading `.` and spaces around the text are ignored (`.DXF`),
/// except when the whole text is a value (a delimiter that is one space).
macro_rules! param_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $text:literal $(| $alias:literal)*),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
        pub enum $name {
            $(
                #[serde(rename = $text)]
                $variant,
            )+
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                use serde::de::Error as _;
                let given = <String as serde::Deserialize>::deserialize(d)?;
                let plain = |s: &str| s.to_ascii_lowercase().replace('-', "_");
                let whole = plain(&given);
                let trimmed = plain(given.trim().trim_start_matches('.'));
                // The whole text first (a delimiter that is one space), then without the dot and
                // the spaces around it.
                for text in [&whole, &trimmed] {
                    $(
                        for name in [$text $(, $alias)*] {
                            if *text == plain(name) {
                                return Ok($name::$variant);
                            }
                        }
                    )+
                }
                Err(D::Error::unknown_variant(&given, &[$($text),+]))
            }
        }
    };
}
pub(crate) use param_enum;
