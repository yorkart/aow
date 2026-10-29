import { api } from '../../lib/api';
import type { Project, ScanResult, Skill, SkillDetail, SkillEntry } from './types';

const root = (org: string) => `/api/orgs/${encodeURIComponent(org)}`;
export const listSkills = (org: string, signal?: AbortSignal) => api<Skill[]>(`${root(org)}/skills`, { signal });
export const getSkill = (org: string, id: string, signal?: AbortSignal) => api<SkillDetail>(`${root(org)}/skills/${encodeURIComponent(id)}`, { signal });
export const createSkill = (org: string, values: { name: string; description: string; instructions: string }) => api<SkillDetail>(`${root(org)}/skills`, { method: 'POST', body: JSON.stringify(values) });
export const saveSkill = (org: string, id: string, version: string, files: SkillEntry[]) => api<SkillDetail>(`${root(org)}/skills/${encodeURIComponent(id)}`, { method: 'PUT', body: JSON.stringify({ base_version_id: version, files }) });
export const deleteSkill = (org: string, id: string) => api<void>(`${root(org)}/skills/${encodeURIComponent(id)}`, { method: 'DELETE' });
export const listProjects = (org: string, signal?: AbortSignal) => api<Project[]>(`${root(org)}/projects`, { signal });
export const createProject = (org: string, name: string, directory: string) => api<Project>(`${root(org)}/projects`, { method: 'POST', body: JSON.stringify({ name, directory }) });
export const scanProject = (org: string, project: string) => api<ScanResult>(`${root(org)}/skills/scan`, { method: 'POST', body: JSON.stringify({ project_id: project }) });
export const importSkills = (org: string, project: string, skills: { path: string; name: string }[]) => api<Skill[]>(`${root(org)}/skills/import`, { method: 'POST', body: JSON.stringify({ project_id: project, skills }) });
