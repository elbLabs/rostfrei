"""Decode the VT cursor/erase operations emitted by this OpenTUI probe.

Not an accessibility/contrast evaluator or a general terminal emulator. Retain
the original ANSI beside frames so this bounded ASCII-focused check is auditable.
"""

import re
import unicodedata


def screen(raw, rows, columns):
    text = raw.decode("utf8", errors="replace")
    text = re.sub(r"\x1b[P_^].*?\x1b\\", "", text, flags=re.DOTALL)
    text = re.sub(r"\x1b\].*?(?:\x07|\x1b\\)", "", text, flags=re.DOTALL)
    cells = [[" "] * columns for _ in range(rows)]
    row = column = 0
    saved = (0, 0)
    index = 0
    while index < len(text):
        char = text[index]
        if char == "\x1b":
            match = re.match(r"\x1b\[([0-?]*)([ -/]*)([@-~])", text[index:])
            if match:
                params, _, command = match.groups()
                values = [int(value) if value.isdecimal() else 0 for value in params.split(";")]
                count = values[0] or 1
                if command in "Hf":
                    row = max(0, min(rows - 1, count - 1))
                    column = max(0, min(columns - 1, (values[1] if len(values) > 1 and values[1] else 1) - 1))
                elif command == "G": column = min(columns - 1, count - 1)
                elif command == "d": row = min(rows - 1, count - 1)
                elif command == "A": row = max(0, row - count)
                elif command == "B": row = min(rows - 1, row + count)
                elif command == "C": column = min(columns - 1, column + count)
                elif command == "D": column = max(0, column - count)
                elif command == "J":
                    if values[0] in (2, 3): cells = [[" "] * columns for _ in range(rows)]
                    elif values[0] == 0:
                        cells[row][column:] = [" "] * (columns - column)
                        for y in range(row + 1, rows): cells[y] = [" "] * columns
                    elif values[0] == 1:
                        for y in range(row): cells[y] = [" "] * columns
                        cells[row][:column + 1] = [" "] * (column + 1)
                elif command == "K":
                    start, end = (0, columns) if values[0] == 2 else (0, column + 1) if values[0] == 1 else (column, columns)
                    cells[row][start:end] = [" "] * (end - start)
                elif command == "X": cells[row][column:min(columns, column + count)] = [" "] * min(count, columns - column)
                elif command == "s": saved = (row, column)
                elif command == "u" and params == "": row, column = saved
                elif command == "S": cells = cells[min(rows, count):] + [[" "] * columns for _ in range(min(rows, count))]
                elif command == "T": cells = [[" "] * columns for _ in range(min(rows, count))] + cells[:max(0, rows - count)]
                index += len(match.group())
                continue
            match = re.match(r"\x1b(?:[()][A-Z0-9]|.)", text[index:])
            index += len(match.group()) if match else 1
            continue
        if char == "\r": column = 0
        elif char == "\n":
            row += 1
            if row >= rows: cells.pop(0); cells.append([" "] * columns); row = rows - 1
        elif char == "\b": column = max(0, column - 1)
        elif char == "\t": column = min(columns - 1, (column // 8 + 1) * 8)
        elif ord(char) >= 32 and ord(char) != 127:
            if unicodedata.combining(char):
                if column: cells[row][min(columns - 1, column - 1)] += char
            else:
                if column >= columns:
                    column = 0; row += 1
                    if row >= rows: cells.pop(0); cells.append([" "] * columns); row = rows - 1
                cells[row][column] = char
                width = 2 if unicodedata.east_asian_width(char) in ("W", "F") else 1
                if width == 2 and column + 1 < columns: cells[row][column + 1] = ""
                column += width
        index += 1
    return ["".join(line).rstrip() for line in cells]
