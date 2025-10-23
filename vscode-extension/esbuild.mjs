import { build, context as esbuildContext } from 'esbuild';
import { fileURLToPath } from 'url';
import { dirname, resolve } from 'path';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const watchMode = process.argv.includes('--watch');

const options = {
  entryPoints: [resolve(__dirname, 'src/extension.ts')],
  bundle: true,
  format: 'cjs',
  platform: 'node',
  target: ['node18'],
  outfile: resolve(__dirname, 'dist/extension.js'),
  sourcemap: true,
  external: ['vscode'],
  logLevel: 'silent'
};

if (watchMode) {
  const ctx = await esbuildContext(options);
  await ctx.watch();
  console.log('esbuild is watching extension sources...');
} else {
  await build(options);
  console.log('Bundled VTCode Companion to dist/extension.js');
}
