import type { Metadata, Viewport } from "next";
import { Space_Grotesk, JetBrains_Mono } from "next/font/google";
import "./globals.css";
import { Providers } from "@/components/providers";

const display = Space_Grotesk({ variable: "--font-display", subsets: ["latin", "latin-ext"] });
const code = JetBrains_Mono({ variable: "--font-code", subsets: ["latin", "latin-ext"] });

export const metadata: Metadata = {
  title: "Galileo",
  description: "Observability and AI control plane",
};

export const viewport: Viewport = {
  themeColor: "#0d0818",
  colorScheme: "dark",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className={`${display.variable} ${code.variable} h-full antialiased`}>
      <body className="min-h-full flex flex-col">
        <script dangerouslySetInnerHTML={{ __html: `try{var l=localStorage.getItem("galileo.locale");if(l)document.documentElement.lang=l;}catch(e){}` }} />
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
