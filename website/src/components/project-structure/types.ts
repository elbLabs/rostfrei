export type StructureNode = {
  id: string
  name: string
  path: string
  kind: "directory" | "file"
  role: string
  summary: string
  allowed: string[]
  guarantee: string
  children?: StructureNode[]
}
