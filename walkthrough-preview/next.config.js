const withMDX = require("@next/mdx")({
  extension: /\.mdx?$/,
  options: {
    providerImportSource: "@mdx-js/react",
    remarkPlugins: [
      [
        require("@code-hike/mdx").remarkCodeHike,
        {
          theme: "github-dark",
          lineNumbers: true,
          showCopyButton: true,
          skipLanguages: [],
        },
      ],
    ],
  },
});

module.exports = withMDX({
  pageExtensions: ["js", "jsx", "mdx"],
  experimental: {
    externalDir: true,
  },
  reactStrictMode: true,
  webpack(config) {
    config.resolve.alias["@mdx-js/react"] = require.resolve("@mdx-js/react");
    config.resolve.alias["@code-hike/mdx/dist/components.cjs.js"] =
      require.resolve("@code-hike/mdx/dist/components.cjs.js");
    config.resolve.alias["@code-hike/mdx/components"] = require.resolve(
      "@code-hike/mdx/components",
    );
    return config;
  },
});
