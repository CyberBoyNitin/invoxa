import type { Metadata, Viewport } from "next";
import { Toaster } from "sonner";
import { Providers } from "@/components/providers";
import "./globals.css";

export const metadata: Metadata = {
  title: {
    default: "Invoxa — Invoice intelligence",
    template: "%s · Invoxa",
  },
  description:
    "Invoxa turns supplier invoices into validated, reviewable financial records with explicit human approval.",
  robots: { index: false, follow: false },
};

export const viewport: Viewport = {
  width: "device-width",
  initialScale: 1,
  themeColor: "#f4f1e9",
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>
        <Providers>{children}</Providers>
        <Toaster
          position="bottom-right"
          toastOptions={{
            style: {
              background: "#1b1c20",
              color: "#fdfcf8",
              border: "1px solid #3a3b40",
              borderRadius: "8px",
              fontSize: "13px",
            },
          }}
        />
      </body>
    </html>
  );
}
