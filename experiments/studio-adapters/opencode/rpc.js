import { Rpc } from "@opencode/plugin/rpc"

export const operationInput = {
  type: "object",
  properties: { captureId: { type: "string" }, snapshotId: { type: "string" }, operationId: { type: "string" } },
  required: ["captureId", "snapshotId", "operationId"],
  additionalProperties: false,
}
export const messageInput = {
  type: "object",
  properties: { ...operationInput.properties, messageId: { type: "string" } },
  required: [...operationInput.required, "messageId"],
  additionalProperties: false,
}
const objectOutput = { type: "object" }
const offset = { type: "integer", minimum: 0 }
export const listInput = { ...operationInput, properties: { ...operationInput.properties, offset }, required: [...operationInput.required, "offset"] }
export const detailInput = { ...messageInput, properties: { ...messageInput.properties, offset, section: { enum: ["identity", "limitations", "payload", "outcome"] }, reveal: { type: "boolean" } }, required: [...messageInput.required, "offset", "section", "reveal"] }
export const sourceInput = { ...operationInput, properties: { ...operationInput.properties, offset, reveal: { type: "boolean" } }, required: [...operationInput.required, "offset", "reveal"] }
export const emptyInput = { type: "object", properties: {}, additionalProperties: false }
const errors = {
  invalid_selection: {
    type: "object", properties: { reason: { type: "string" } }, required: ["reason"], additionalProperties: false,
  },
}

export const Presentation = Rpc.define({
  id: "rostfrei.presentation.spike",
  events: {},
  methods: {
    listCaptures: { input: emptyInput, output: { type: "array", items: objectOutput } },
    showOperation: { input: operationInput, output: objectOutput, errors },
    listMessages: { input: listInput, output: objectOutput, errors },
    captureDetail: { input: listInput, output: objectOutput, errors },
    renderOperation: { input: operationInput, output: { type: "string" }, errors },
    inspectMessage: { input: messageInput, output: objectOutput, errors },
    messageDetail: { input: detailInput, output: objectOutput, errors },
    sourcePage: { input: sourceInput, output: objectOutput, errors },
    selectMessage: { input: messageInput, output: objectOutput, errors },
    selectedMessage: { input: emptyInput, output: { anyOf: [objectOutput, { type: "null" }] } },
    diagnostics: { input: emptyInput, output: objectOutput },
  },
})
