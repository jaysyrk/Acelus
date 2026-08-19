pub mod arguments;
pub mod manifest;
pub mod rules;
pub mod version;

pub use arguments::{
    substitute, Argument, ResolvedArguments, Substitutions, UnresolvedPlaceholders,
};
pub use manifest::{VersionManifest, VersionSummary};
pub use rules::{evaluate, Action, Arch, Environment, Features, Os, Rule};
pub use version::{
    Artifact, Coordinates, Library, ReleaseType, ResolvedArtifact, ResolvedLibraries,
    ResolvedNative, VersionJson,
};
