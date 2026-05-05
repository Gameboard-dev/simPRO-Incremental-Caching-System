/// Network used to allow containers to communicate.
pub(crate) const DOCKER_NETWORK: &str = "simpro-schema-net";

/// Name of the Docker image containing Diesel and related tooling.
pub(crate) const DOCKER_IMAGE: &str = "simpro-schema-tools";

/// Path to the Dockerfile used to build `TOOLS_IMAGE`.
pub(crate) const DOCKERFILE: &str = "xtask/Dockerfile";
