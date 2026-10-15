import js from '@eslint/js';
import ts from 'typescript-eslint';
import globals from 'globals';
export default [
  {ignores:['dist/**','node_modules/**','apps/web/public/**','packages/schema/**']},
  js.configs.recommended,...ts.configs.recommended,
  {files:['**/*.{ts,tsx,js,mjs}'],languageOptions:{globals:{...globals.browser,...globals.node}},rules:{'@typescript-eslint/no-unused-vars':['error',{argsIgnorePattern:'^_',caughtErrors:'none'}],'no-undef':'off','no-empty':['error',{allowEmptyCatch:true}]}}
];
