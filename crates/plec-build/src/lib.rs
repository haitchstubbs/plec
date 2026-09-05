//! Artifact assembly for compiled Plec applications: the shared build
//! pipeline (clean -> compile artifacts -> browser bundle -> dependency
//! validation -> revision -> brotli -> server bundle -> document) plus its
//! stages. The CLI's build subcommand is a thin argument parser over
//! [`build`]; nothing here shells out to a second compiler.

pub mod modules;

pub use modules::build::{build, BuildError, BuildOptions, BuildResult, Stage};
