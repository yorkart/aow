import {
  Apple, Banana, Bird, Bug, Cat, Cherry, Citrus, Clover, Dog, Fish, Flower, Flower2,
  GitBranch, Grape, Leaf, Rabbit, Rat, Snail, Sprout, Squirrel, TreeDeciduous, TreePalm,
  TreePine, Trees, Turtle, Wheat, createLucideIcon, type LucideIcon, type LucideProps,
} from 'lucide-react';
import type { WorktreeColor, WorktreeIconId } from './types';

export const worktreeColors: { id: WorktreeColor; label: string; value?: string }[] = [
  { id: 'default', label: '默认' },
  { id: 'blue', label: '蓝色', value: '#3b82f6' },
  { id: 'purple', label: '紫色', value: '#a855f7' },
  { id: 'pink', label: '粉色', value: '#ec4899' },
  { id: 'red', label: '红色', value: '#ef4444' },
  { id: 'orange', label: '橙色', value: '#f97316' },
  { id: 'yellow', label: '黄色', value: '#f5b700' },
  { id: 'green', label: '绿色', value: '#22c55e' },
  { id: 'teal', label: '青色', value: '#14b8a6' },
  { id: 'gray', label: '灰色', value: '#a3aab6' },
];
export const worktreeColorValues = Object.fromEntries(
  worktreeColors.map((color) => [color.id, color.value ?? '#79a9eb']),
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
  default: { label: '默认图标', component: GitBranch },
  cat: { label: '猫', component: Cat },
  dog: { label: '狗', component: Dog },
  rabbit: { label: '兔子', component: Rabbit },
  bird: { label: '鸟', component: Bird },
  fish: { label: '鱼', component: Fish },
  turtle: { label: '乌龟', component: Turtle },
  squirrel: { label: '松鼠', component: Squirrel },
  snail: { label: '蜗牛', component: Snail },
  bug: { label: '甲虫', component: Bug },
  rat: { label: '老鼠', component: Rat },
  apple: { label: '苹果', component: Apple },
  banana: { label: '香蕉', component: Banana },
  cherry: { label: '樱桃', component: Cherry },
  citrus: { label: '柑橘', component: Citrus },
  grape: { label: '葡萄', component: Grape },
  pear: { label: '梨', component: Pear },
  peach: { label: '桃子', component: Peach },
  strawberry: { label: '草莓', component: Strawberry },
  watermelon: { label: '西瓜', component: Watermelon },
  pineapple: { label: '菠萝', component: Pineapple },
  leaf: { label: '叶子', component: Leaf },
  sprout: { label: '幼苗', component: Sprout },
  flower: { label: '花朵', component: Flower },
  flower_2: { label: '小花', component: Flower2 },
  clover: { label: '四叶草', component: Clover },
  wheat: { label: '麦穗', component: Wheat },
  tree_pine: { label: '松树', component: TreePine },
  tree_deciduous: { label: '阔叶树', component: TreeDeciduous },
  tree_palm: { label: '棕榈树', component: TreePalm },
  trees: { label: '树林', component: Trees },
};

export const worktreeIconGroups: { label: string; icons: WorktreeIconId[] }[] = [
  { label: '动物', icons: ['cat', 'dog', 'rabbit', 'bird', 'fish', 'turtle', 'squirrel', 'snail', 'bug', 'rat'] },
  { label: '水果', icons: ['apple', 'banana', 'cherry', 'citrus', 'grape', 'pear', 'peach', 'strawberry', 'watermelon', 'pineapple'] },
  { label: '植物', icons: ['leaf', 'sprout', 'flower', 'flower_2', 'clover', 'wheat', 'tree_pine', 'tree_deciduous', 'tree_palm', 'trees'] },
];

export function worktreeIconLabel(icon: WorktreeIconId = 'default') {
  return (icons[icon] ?? icons.default).label;
}

export function WorktreeIcon({ icon = 'default', ...props }: LucideProps & { icon?: WorktreeIconId }) {
  const Icon = (icons[icon] ?? icons.default).component;
  return <Icon aria-hidden="true" {...props} />;
}
