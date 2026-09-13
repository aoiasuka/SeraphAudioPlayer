import tailwindcss from "tailwindcss";
import autoprefixer from "autoprefixer";

// 支持的 WebView2 使用 WOFF2；保留所有 Unicode 切片，仅移除重复 WOFF 源。
const woff2Fonts = {
  postcssPlugin: "seraph-woff2-fonts",
  Once(root) {
    // 必须早于 Vite 的 URL 资源收集；Declaration visitor 会晚到 URL 已变成占位符。
    root.walkAtRules("font-face", (rule) => rule.walkDecls("src", (declaration) => {
      if (!declaration.value.includes("woff2")) return;
      declaration.value = declaration.value.replace(
        /,\s*url\([^)]*\.woff['"]?\)\s*format\(['"]?woff['"]?\)/g,
        ""
      );
    }));
  },
};

export default { plugins: [woff2Fonts, tailwindcss(), autoprefixer()] };
