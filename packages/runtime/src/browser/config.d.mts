export function browserCandidates(env?: NodeJS.ProcessEnv): { path: string; source: string }[];
export function resolveBrowserExecutable(settingPath: string, env?: NodeJS.ProcessEnv): { path?: string; source: string };
export function browserEnvironment(directory: string): Promise<{ keepOpen: boolean; window: 'normal' | 'maximized' | 'fullscreen'; executablePath: string; source: string }>;