import * as esbuild from "esbuild";

const production = process.argv.includes('--production');
const watch = process.argv.includes('--watch');

/**
 * @type {import('esbuild').Plugin}
 */
const esbuildProblemMatcherPlugin = {
	name: 'esbuild-problem-matcher',
	setup(build) {
		build.onStart(() => {
			console.log('[watch] build started');
		});
		build.onEnd((result) => {
			result.errors.forEach(error => console.error(`[ERROR] ${error.text}`));
			console.log('[watch] build finished');
		});
	}
};

async function runBuild() {
	const ctx = await esbuild.context({
		entryPoints: [
			'src/extension.ts'
		],
		bundle: true,
		format: 'cjs',
		minify: production,
		sourcemap: !production,
		sourcesContent: false,
		platform: 'node',
		outbase: '.',
		outdir: 'dist',
                external: ['vscode', 'node-pty'],
		logLevel: 'silent', // Disable esbuild's own logging since we handle it with the plugin
		plugins: [
			esbuildProblemMatcherPlugin,
		],
		define: {
			'process.env.NODE_ENV': JSON.stringify(production ? 'production' : 'development'),
		},
		loader: {
			'.ts': 'ts',
		}
	});

	if (watch) {
		await ctx.watch();
	} else {
		await ctx.rebuild();
		await ctx.dispose();
	}
}

runBuild().catch(e => {
	console.error(e);
	process.exit(1);
});