import { defineConfig } from "vitepress";

export default defineConfig({
  title: "PianoPro",
  description: "Flashy Synthesia Like Software For Linux, Windows and MacOs",
  base: "/piano-pro/",
  themeConfig: {
    nav: [
      { text: "Home", link: "/" },
      { text: "How-to", link: "/pages/installation" },
    ],

    sidebar: [
      {
        text: "How-to",
        items: [
          { text: "Installation", link: "/pages/installation" },
          { text: "Shortcuts", link: "/pages/shortcuts" },
          { text: "Customization", link: "/pages/customization" },
          { text: "Video Encoding", link: "/pages/video-encoding" },
        ],
      },
    ],

    socialLinks: [
      { icon: "github", link: "https://github.com/Quentin-Piot/piano-pro" },
    ],
  },
});
