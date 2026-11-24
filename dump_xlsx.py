import io
from openpyxl import load_workbook
p = r"D:\Codes\GPU\refs\GPU指令集.xlsx"
wb = load_workbook(p, data_only=True)
out = io.StringIO()
for ws in wb.worksheets:
    out.write(f"=== sheet: {ws.title} dims: {ws.dimensions}\n")
    for row in ws.iter_rows():
        vals = []
        for c in row:
            if c.value is not None:
                vals.append(f"{c.coordinate}={c.value}")
        if vals:
            out.write(" | ".join(vals) + "\n")
with open(r"D:\Codes\GPU\dump_xlsx.txt", "w", encoding="utf-8") as f:
    f.write(out.getvalue())
print("written")