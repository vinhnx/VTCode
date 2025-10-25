import * as path from 'path';
import { runTests } from '@vscode/test-electron';

async function main() {
    try {
        const extensionDevelopmentPath = path.resolve(__dirname, '../../');
        const extensionTestsPath = path.resolve(__dirname, './suite/index');

        await runTests({ extensionDevelopmentPath, extensionTestsPath, version: 'stable' });
    } catch (error) {
        console.error('Failed to run VS Code tests:', error);
        process.exit(1);
    }
}

void main();
