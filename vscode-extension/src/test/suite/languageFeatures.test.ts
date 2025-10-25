import * as assert from 'assert';
import * as vscode from 'vscode';

suite('VTCode configuration language features', () => {
    suiteSetup(async () => {
        const extension = vscode.extensions.getExtension('vtcode.vtcode-companion');
        assert.ok(extension, 'VTCode Companion extension not found');
        await extension.activate();
    });

    test('provides completions for agent keys', async () => {
        const document = await vscode.workspace.openTextDocument({ language: 'toml', content: '[agent]\n' });
        const vtcodeDocument = await vscode.languages.setTextDocumentLanguage(document, 'vtcode-config');
        await vscode.window.showTextDocument(vtcodeDocument);

        const position = new vscode.Position(1, 0);
        const completionList = await vscode.commands.executeCommand<vscode.CompletionList>(
            'vscode.executeCompletionItemProvider',
            vtcodeDocument.uri,
            position
        );

        assert.ok(completionList, 'Expected completions for vtcode.toml');
        const labels = completionList?.items.map((item) => (typeof item.label === 'string' ? item.label : item.label.label));
        assert.ok(labels?.includes('provider'), 'Expected provider completion in agent section');
    });

    test('shows hover text for agent section headers', async () => {
        const document = await vscode.workspace.openTextDocument({ language: 'toml', content: '[agent]\n' });
        const vtcodeDocument = await vscode.languages.setTextDocumentLanguage(document, 'vtcode-config');

        const hovers = await vscode.commands.executeCommand<vscode.Hover[]>(
            'vscode.executeHoverProvider',
            vtcodeDocument.uri,
            new vscode.Position(0, 1)
        );

        assert.ok(hovers && hovers.length > 0, 'Expected hover information for agent section');
        const hoverText = hovers
            .flatMap((hover) => hover.contents)
            .map((content) => (typeof content === 'string' ? content : content.value))
            .join('\n');
        assert.ok(hoverText.includes('Core VTCode agent behavior'), 'Hover text should describe agent section');
    });

    test('creates document symbols for configuration sections', async () => {
        const document = await vscode.workspace.openTextDocument({
            language: 'toml',
            content: '[agent]\nprovider = "openai"\n\n[agent.onboarding]\nenabled = true\n'
        });
        const vtcodeDocument = await vscode.languages.setTextDocumentLanguage(document, 'vtcode-config');

        const symbols = await vscode.commands.executeCommand<vscode.DocumentSymbol[]>(
            'vscode.executeDocumentSymbolProvider',
            vtcodeDocument.uri
        );

        assert.ok(symbols && symbols.length > 0, 'Expected document symbols for vtcode.toml');
        const symbolNames = flattenSymbols(symbols).map((symbol) => symbol.name);
        assert.ok(symbolNames.includes('Agent'), 'Document symbols should include the Agent section');
        assert.ok(symbolNames.includes('Onboarding'), 'Document symbols should include nested sections');
    });
});

function flattenSymbols(symbols: vscode.DocumentSymbol[]): vscode.DocumentSymbol[] {
    return symbols.flatMap((symbol) => [symbol, ...flattenSymbols(symbol.children)]);
}
