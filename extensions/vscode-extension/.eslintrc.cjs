module.exports = {
    root: true,
    parser: '@typescript-eslint/parser',
    parserOptions: {
        project: ['./tsconfig.json']
    },
    plugins: ['@typescript-eslint'],
    extends: [
        'eslint:recommended',
        'plugin:@typescript-eslint/recommended'
    ],
    ignorePatterns: ['dist', 'node_modules'],
    rules: {
        indent: ['error', 4],
        quotes: ['error', 'single'],
        semi: ['error', 'always'],
        'max-len': ['error', { code: 100 }]
    }
};
