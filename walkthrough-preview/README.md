# Walkthrough Preview

This is an isolated Next.js/Code Hike preview app for `../docs/walkthrough.mdx`.
It is intentionally separate from the Rust workspace so Node dependencies and
preview configuration stay out of the main project root.

Run it with:

```bash
npm install
npm run dev -- --hostname 127.0.0.1 --port 3027
```

Then open <http://127.0.0.1:3027>.

Use this preview when editing the walkthrough prose, Code Hike focus ranges, or
the highlighted Rust code sections.
