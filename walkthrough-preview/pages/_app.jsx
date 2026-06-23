import { MDXProvider } from "@mdx-js/react";
import { CH } from "@code-hike/mdx/components";
import "@code-hike/mdx/dist/index.css";
import "../styles.css";

const components = { CH };

export default function App({ Component, pageProps }) {
  return (
    <MDXProvider components={components}>
      <Component {...pageProps} />
    </MDXProvider>
  );
}
