"""Generate deterministic, entirely synthetic invoice fixtures for local demos.

These are not real invoices. Values are invented for testing Invoxa's upload,
OCR, extraction, validation, duplicate, review, approval and export flows.
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent
ROOT.mkdir(parents=True, exist_ok=True)

LINES = [
    "TAX INVOICE",
    "Acme Cloud Services Pvt Ltd",
    "Vendor: Acme Cloud Services Pvt Ltd",
    "GSTIN: 29AABCA1234F1Z5",
    "Email: billing@acme-cloud.example",
    "Invoice Number: INV-2026-0042",
    "Invoice Date: 2026-10-01",
    "Due Date: 2026-10-31",
    "Currency: INR",
    "Purchase Order: PO-7781",
    "Payment Terms: Net 30",
    "",
    "Description Qty Unit Price Amount",
    "Cloud hosting 1 15000.00 15000.00",
    "Support plan 2 2500.00 5000.00",
    "",
    "Subtotal: 20000.00",
    "CGST 9% 1800.00",
    "SGST 9% 1800.00",
    "Tax Total: 3600.00",
    "Total: 23600.00",
    "Balance Due: 23600.00",
]


def escape_pdf(text: str) -> str:
    return text.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")


def write_pdf(path: Path, lines, footer: str = "") -> None:
    content_lines = ["BT", "/F1 11 Tf", "50 760 Td", "15 TL"]
    for line in lines:
        content_lines.append(f"({escape_pdf(line)}) Tj T*")
    if footer:
        content_lines.append(f"({escape_pdf(footer)}) Tj T*")
    content_lines.append("ET")
    stream = "\n".join(content_lines).encode("latin-1")

    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] "
        b"/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        b"<< /Length " + str(len(stream)).encode() + b" >>\nstream\n" + stream + b"\nendstream",
    ]
    data = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = [0]
    for index, body in enumerate(objects, start=1):
        offsets.append(len(data))
        data.extend(f"{index} 0 obj\n".encode())
        data.extend(body)
        data.extend(b"\nendobj\n")
    xref = len(data)
    data.extend(f"xref\n0 {len(objects) + 1}\n".encode())
    data.extend(b"0000000000 65535 f \n")
    for offset in offsets[1:]:
        data.extend(f"{offset:010d} 00000 n \n".encode())
    data.extend(
        (
            f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\n"
            f"startxref\n{xref}\n%%EOF\n"
        ).encode()
    )
    path.write_bytes(bytes(data))


write_pdf(ROOT / "acme_invoice.pdf", LINES, "Synthetic demo fixture - not a real invoice")
# A different file hash with the same vendor, number and amount. It must be
# detected as a possible duplicate rather than silently rejected.
write_pdf(ROOT / "acme_invoice_duplicate.pdf", LINES, "Duplicate demo copy - synthetic")

font_path = Path("C:/Windows/Fonts/arial.ttf")
if not font_path.exists():
    font_path = Path("C:/Windows/Fonts/segoeui.ttf")
font = ImageFont.truetype(str(font_path), 26)
small = ImageFont.truetype(str(font_path), 22)
image = Image.new("RGB", (1650, 2200), "white")
draw = ImageDraw.Draw(image)
y = 100
for index, line in enumerate(LINES):
    if line:
        draw.text((100, y), line, fill="black", font=font if index < 4 else small)
    y += 56
draw.text((100, y + 20), "Synthetic scanned demo fixture - not a real invoice", fill="black", font=small)
image.save(ROOT / "acme_invoice_scan.png", optimize=True)
print("Wrote:", *(p.name for p in ROOT.glob("acme_invoice*")))
