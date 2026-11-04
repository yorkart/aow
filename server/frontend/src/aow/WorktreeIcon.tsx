import { t } from '../lib/i18n';
import {
  Apple, Banana, Bird, Bug, Cat, Cherry, Citrus, Clover, Dog, Fish, Flower, Flower2, FolderGit2,
  Grape, Leaf, Rabbit, Rat, Snail, Sprout, Squirrel, TreeDeciduous, TreePalm,
  TreePine, Trees, Turtle, Wheat, createLucideIcon, type LucideIcon, type LucideProps,
} from 'lucide-react';
import type { WorktreeColor, WorktreeIconId } from './types';

export const worktreeColors: { id: WorktreeColor; label: string; value?: string }[] = [
  { id: 'default', label: t("aow.default") },
  { id: 'blue', label: t("aow.blue"), value: '#3b82f6' },
  { id: 'purple', label: t("aow.purple"), value: '#a855f7' },
  { id: 'pink', label: t("aow.pink"), value: '#ec4899' },
  { id: 'red', label: t("aow.red"), value: '#ef4444' },
  { id: 'orange', label: t("aow.orange"), value: '#f97316' },
  { id: 'yellow', label: t("aow.yellow"), value: '#f5b700' },
  { id: 'green', label: t("aow.green"), value: '#22c55e' },
  { id: 'teal', label: t("aow.cyan"), value: '#14b8a6' },
  { id: 'gray', label: t("aow.gray"), value: '#a3aab6' },
];
export const worktreeColorValues = Object.fromEntries(
  worktreeColors.map((color) => [color.id, color.value ?? 'var(--aow-icon)']),
) as Record<WorktreeColor, string>;

// Match Lucide's 24px viewBox, rounded strokes and currentColor for the remaining fruits.
const Pear = createLucideIcon('Pear', [
  ['path', { d: 'M12 5c-3 0-2 5-5 8-5 5-2 9 5 9s10-4 5-9c-3-3-2-8-5-8Z', key: 'fruit' }],
  ['path', { d: 'M12 5V2m0 3c0-3 3-3 5-2-1 3-3 3-5 2', key: 'leaf' }],
]);
const Peach = createLucideIcon('Peach', [
  ['path', { d: 'M12 7C3 2 0 12 6 18l6 4 6-4c6-6 3-16-6-11Z', key: 'fruit' }],
  ['path', { d: 'M12 7c-3 4 3 8 0 15m0-15V3m0 2c2-4 6-3 7-2-1 3-5 4-7 2', key: 'detail' }],
]);
const Strawberry = createLucideIcon('Strawberry', [
  ['path', { d: 'M5 8C-1 12 8 22 12 22s13-10 7-14M12 9 6 6l4-1 2-3 2 3 4 1-6 3Z', key: 'fruit' }],
  ['path', { d: 'M7 12h.01M12 13h.01M17 12h.01M9 17h.01M15 17h.01', key: 'seeds' }],
]);
const Watermelon = createLucideIcon('Watermelon', [
  ['path', { d: 'M2 8h20a10 10 0 0 1-20 0Zm3 0a7 7 0 0 0 14 0', key: 'fruit' }],
  ['path', { d: 'M8 10v1m4 1v1m4-3v1', key: 'seeds' }],
]);
const Pineapple = createLucideIcon('Pineapple', [
  ['path', { d: 'm9 8-3-5 4 2 2-4 2 4 4-2-3 5', key: 'leaves' }],
  ['rect', { x: '6', y: '8', width: '12', height: '14', rx: '6', key: 'fruit' }],
  ['path', { d: 'm7 11 10 8M7 17l5 4m0-12 5 4M7 19l10-8m-5 10 5-4M7 13l5-4', key: 'texture' }],
]);

const icons: Record<WorktreeIconId, { label: string; component: LucideIcon }> = {
  default: { label: t("aow.default_icon"), component: FolderGit2 },
  cat: { label: t("aow.cat"), component: Cat },
  dog: { label: t("aow.dog"), component: Dog },
  rabbit: { label: t("aow.rabbit"), component: Rabbit },
  bird: { label: t("aow.bird"), component: Bird },
  fish: { label: t("aow.fish"), component: Fish },
  turtle: { label: t("aow.turtle"), component: Turtle },
  squirrel: { label: t("aow.squirrel"), component: Squirrel },
  snail: { label: t("aow.snail"), component: Snail },
  bug: { label: t("aow.beetle"), component: Bug },
  rat: { label: t("aow.mouse"), component: Rat },
  apple: { label: t("aow.apple"), component: Apple },
  banana: { label: t("aow.banana"), component: Banana },
  cherry: { label: t("aow.cherry"), component: Cherry },
  citrus: { label: t("aow.citrus"), component: Citrus },
  grape: { label: t("aow.grapes"), component: Grape },
  pear: { label: t("aow.pear"), component: Pear },
  peach: { label: t("aow.peach"), component: Peach },
  strawberry: { label: t("aow.strawberry"), component: Strawberry },
  watermelon: { label: t("aow.watermelon"), component: Watermelon },
  pineapple: { label: t("aow.pineapple"), component: Pineapple },
  leaf: { label: t("aow.leaf"), component: Leaf },
  sprout: { label: t("aow.seedling"), component: Sprout },
  flower: { label: t("aow.flower"), component: Flower },
  flower_2: { label: t("aow.blossom"), component: Flower2 },
  clover: { label: t("aow.four_leaf_clover"), component: Clover },
  wheat: { label: t("aow.wheat"), component: Wheat },
  tree_pine: { label: t("aow.pine_tree"), component: TreePine },
  tree_deciduous: { label: t("aow.deciduous_tree"), component: TreeDeciduous },
  tree_palm: { label: t("aow.palm_tree"), component: TreePalm },
  trees: { label: t("aow.forest"), component: Trees },
};

export const worktreeIconGroups: { label: string; icons: WorktreeIconId[] }[] = [
  { label: t("aow.animals"), icons: ['cat', 'dog', 'rabbit', 'bird', 'fish', 'turtle', 'squirrel', 'snail', 'bug', 'rat'] },
  { label: t("aow.fruit"), icons: ['apple', 'banana', 'cherry', 'citrus', 'grape', 'pear', 'peach', 'strawberry', 'watermelon', 'pineapple'] },
  { label: t("aow.plants"), icons: ['leaf', 'sprout', 'flower', 'flower_2', 'clover', 'wheat', 'tree_pine', 'tree_deciduous', 'tree_palm', 'trees'] },
];

export function worktreeIconLabel(icon: WorktreeIconId = 'default') {
  return (icons[icon] ?? icons.default).label;
}

export function WorktreeIcon({ icon = 'default', ...props }: LucideProps & { icon?: WorktreeIconId }) {
  const Icon = (icons[icon] ?? icons.default).component;
  return <Icon aria-hidden="true" {...props} />;
}
