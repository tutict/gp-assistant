// Vite client types for CSS module side-effect imports
declare module "*.css" {
  const content: Record<string, string>;
  export default content;
}
