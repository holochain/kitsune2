# Website

This website is built using [Docusaurus](https://docusaurus.io/), a modern static website generator.

## Installation

```bash
yarn
```

## Local Development

```bash
yarn start
```

This command starts a local development server and opens up a browser window. Most changes are reflected live without having to restart the server.

## Build

```bash
yarn build
```

This command generates static content into the `build` directory and can be served using any static contents hosting service.

## Deployment

The `holochain/kitsune2` GitHub repo is set up with continuous deployment to Netlify Pages; as soon as a PR is merged to `main`, it'll be live at the URL https://kitsune2.holochain.org .