FROM mcr.microsoft.com/devcontainers/rust:2-1-trixie

# Install Node.js via NodeSource
ARG NODE_VERSION=24
RUN curl -fsSL https://deb.nodesource.com/setup_${NODE_VERSION}.x | bash - \
    && apt-get install -y nodejs \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Install pandoc
RUN apt-get update \
    && apt-get install -y pandoc \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Enable corepack (ships with Node.js 16+)
RUN corepack enable

