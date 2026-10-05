//! Artifact assembly for compiled Plec applications: the shared build
//! pipeline (clean -> compile Plec artifacts -> Vite browser build/validation
//! -> revision -> brotli -> server bundle -> manifests -> document) plus its
//! stages. The CLI's build subcommand is a thin argument parser over
//! [`build`]; nothing here shells out to a second compiler.

pub mod modules;

pub use modules::build::{BuildError, BuildOptions, BuildResult, RuntimeSource, Stage, build};
