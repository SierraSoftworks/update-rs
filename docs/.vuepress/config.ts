import { defineUserConfig } from "vuepress";
import { viteBundler } from "@vuepress/bundler-vite";
import { defaultTheme } from "@vuepress/theme-default";

export default defineUserConfig({
    lang: "en-GB",
    title: "update-rs",
    description:
        "Self-contained, in-place self-updates for Rust applications — download the new release, relaunch, and replace the running binary.",

    head: [
        [
            "meta",
            {
                name: "description",
                content:
                    "Documentation for update-rs, a self-contained in-place self-update library for Rust applications.",
            },
        ],
        ["link", { rel: "icon", href: "/icon.svg" }],
    ],

    bundler: viteBundler(),

    theme: defaultTheme({
        logo: "/icon.svg",

        repo: "SierraSoftworks/update-rs",
        docsRepo: "SierraSoftworks/update-rs",
        docsDir: "docs",

        navbar: [
            {
                text: "Guide",
                link: "/guide/",
            },
            {
                text: "API (docs.rs)",
                link: "https://docs.rs/update-rs",
                target: "_blank",
            },
            {
                text: "crates.io",
                link: "https://crates.io/crates/update-rs",
                target: "_blank",
            },
            {
                text: "Report an Issue",
                link: "https://github.com/SierraSoftworks/update-rs/issues/new",
                target: "_blank",
            },
        ],

        sidebar: {
            "/guide/": [
                {
                    text: "Guide",
                    children: [
                        "/guide/README.md",
                        "/guide/usage.md",
                        "/guide/how-it-works.md",
                        "/guide/windows.md",
                        "/guide/release-pipeline.md",
                    ],
                },
            ],
        },
    }),
});
