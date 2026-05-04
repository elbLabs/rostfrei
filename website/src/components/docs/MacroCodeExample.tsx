import { MdxCodeBlock } from "./MdxCodeBlock"
import { getMacroCode, type MacroExampleName } from "@/docs/macro-code-data"

export function MacroCodeExample({ name }: { name: MacroExampleName }) {
  return (
    <MdxCodeBlock>
      <code className="language-rust">{getMacroCode(name)}</code>
    </MdxCodeBlock>
  )
}
