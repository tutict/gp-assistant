// Vite client types for CSS module side-effect imports
interface ImportMetaEnv {
  readonly DEV: boolean;
  readonly VITE_PERF_DIAGNOSTICS?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

declare module "*.css" {
  const content: Record<string, string>;
  export default content;
}
