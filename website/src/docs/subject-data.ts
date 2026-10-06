import source from "../../../docs/messaging/subjects.mdx?raw"

// Enhance the canonical Markdown table rather than maintain a second reference.
const table = /<SubjectExplorer>\s*([\s\S]*?)\s*<\/SubjectExplorer>/.exec(source)?.[1]
if (!table) throw new Error("Missing subject reference table in shared documentation")

export const subjectFamilies = table.split(/\r?\n/)
  .filter((line) => line.startsWith("|"))
  .slice(2)
  .map((line) => {
    const cells = line.split("|").slice(1, -1).map((cell) => cell.trim())
    if (cells.length !== 5) throw new Error(`Invalid subject reference row: ${line}`)
    const [title, description, subject, filters, stream] = cells
    return {
      title,
      description,
      subject: subject.replaceAll("`", ""),
      filters: [...filters.matchAll(/`([^`]+)`/g)].map((match) => match[1]),
      stream: stream === "None" ? null : stream.replaceAll("`", ""),
    }
  })
