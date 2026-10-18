const projects = new Map<string, string>([
  ['plec', 'Plec compiler and runtime'],
  ['p42', 'Project 42'],
]);

export function getProjectName(id: string): string | undefined {
  return projects.get(id);
}

export function setProjectName(id: string, name: string): boolean {
  if (!projects.has(id)) return false;
  projects.set(id, name);
  return true;
}
