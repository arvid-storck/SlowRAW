#![deny(unstable_features)]
#![allow(macro_expanded_macro_exports_accessed_by_absolute_paths)]

include!(concat!(
    env!("SLOWRAW_RAWLER_SOURCE"),
    "/src/slowraw-lib.rs"
));
