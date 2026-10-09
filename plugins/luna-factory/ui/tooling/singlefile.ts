import type { Plugin } from "vite";

export function inlineAssets(html: string, assets: ReadonlyMap<string, string>): string {
  const read = (path: string): string => {
    const value = assets.get(path.replace(/^\//, ""));
    if (value === undefined) throw new Error(`Unresolved build asset: ${path}`);
    return value;
  };
  return html
    .replace(/<script\b[^>]*\bsrc="([^"]+)"[^>]*><\/script>/g, (_match: string, path: string) => `<script type="module">${read(path).replace(/<\/script/gi, "<\\/script")}</script>`)
    .replace(/<link\b[^>]*\bhref="([^"]+\.css)"[^>]*>/g, (_match: string, path: string) => `<style>${read(path).replace(/<\/style/gi, "<\\/style")}</style>`);
}

export function singleFile(): Plugin {
  return {
    name: "luna-single-file-resource",
    enforce: "post",
    generateBundle(_options, bundle) {
      const html = bundle["index.html"];
      if (!html || html.type !== "asset") throw new Error("Missing workbench HTML");
      const assets = new Map<string, string>();
      for (const [name, output] of Object.entries(bundle)) {
        if (name === "index.html") continue;
        if (output.type === "chunk") assets.set(name, output.code);
        else if (name.endsWith(".css")) assets.set(name, typeof output.source === "string" ? output.source : new TextDecoder().decode(output.source));
        else throw new Error(`Unexpected external workbench asset: ${name}`);
      }
      html.source = inlineAssets(typeof html.source === "string" ? html.source : new TextDecoder().decode(html.source), assets);
      for (const name of assets.keys()) delete bundle[name];
    },
  };
}
