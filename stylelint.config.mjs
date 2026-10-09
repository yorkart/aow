export default {
  // Check CSS correctness while preserving the repository's compact formatting
  // and intentional cascade ordering.
  extends: ['stylelint-config-recommended'],
  rules: { 'no-descending-specificity': null },
  ignoreFiles: ['**/node_modules/**', '**/dist/**'],
};
