import { defineConfig, type Plugin } from 'vite';
import { copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const rootDir = dirname(fileURLToPath(import.meta.url));
const uiDir = join(rootDir, 'ui');
const distDir = join(rootDir, 'dist');

function nexusStaticAssetsPlugin(): Plugin {
  return {
    name: 'nexus-static-assets',
    closeBundle() {
      mkdirSync(distDir, { recursive: true });
      const stylesSrc = join(uiDir, 'styles.css');
      if (existsSync(stylesSrc)) {
        copyFileSync(stylesSrc, join(distDir, 'styles.css'));
      }
      const overlayCssSrc = join(uiDir, 'js', 'features', 'circle-search', 'overlay.css');
      if (existsSync(overlayCssSrc)) {
        const outSub = join(distDir, 'js', 'features', 'circle-search');
        mkdirSync(outSub, { recursive: true });
        copyFileSync(overlayCssSrc, join(outSub, 'overlay.css'));
      }
      const logoSrc = join(uiDir, 'assets', 'nexus-logo.png');
      if (existsSync(logoSrc)) {
        const outAssets = join(distDir, 'assets');
        mkdirSync(outAssets, { recursive: true });
        copyFileSync(logoSrc, join(outAssets, 'nexus-logo.png'));
      }
    },
  };
}

export default defineConfig({
  root: uiDir,
  base: './',
  publicDir: false,
  plugins: [nexusStaticAssetsPlugin()],
  build: {
    outDir: distDir,
    emptyOutDir: true,
    target: 'es2022',
    rollupOptions: {
      input: {
        settings: resolve(uiDir, 'settings.html'),
        overlay: resolve(uiDir, 'circle-search-overlay.html'),
      },
    },
  },
  server: {
    port: 5174,
    strictPort: true,
  },
});
