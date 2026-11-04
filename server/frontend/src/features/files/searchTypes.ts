export type SearchMode = 'name' | 'content';
export interface SearchOptions {
  mode: SearchMode;
  case_sensitive: boolean;
  whole_word: boolean;
  use_regex: boolean;
  include_ignored: boolean;
  include_pattern: string;
  exclude_pattern: string;
}
export interface SearchMatch {
  line: number;
  column: number;
  match_length: number;
  text: string;
  preview_column: number;
  preview_length: number;
}
export interface SearchFile { path: string; relative_path: string; matches: SearchMatch[] }
export interface SearchResult { files: SearchFile[]; total_matches: number; truncated: boolean }
export interface SearchState { key: string; loading: boolean; result?: SearchResult; error?: string }
export type OpenSearchFile = (path: string, source: 'project' | 'notes', match?: SearchMatch) => void;
