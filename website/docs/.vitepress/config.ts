import { defineConfig } from 'vitepress'
import locales from './locales/index.ts'

const BASE = '/device_faker/'
const SITE = 'https://seyud.github.io/device_faker/'

export default defineConfig({
  title: 'Device Faker',
  base: BASE,
  locales: locales.locales,
  lastUpdated: true,
  cleanUrls: false,

  sitemap: {
    hostname: SITE
  },

  head: [
    // head 里的 URL 不会被自动补 base，必须手写前缀
    ['link', { rel: 'icon', type: 'image/png', href: `${BASE}favicon.png` }],
    ['link', { rel: 'apple-touch-icon', href: `${BASE}apple-touch-icon.png` }],
    ['meta', { name: 'theme-color', content: '#4f7cff' }],

    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:site_name', content: 'Device Faker' }],
    ['meta', { property: 'og:title', content: 'Device Faker — 基于 Zygisk 的机型伪装模块' }],
    ['meta', { property: 'og:description', content: '为每个应用配置不同的设备型号，基于 Zygisk 的机型伪装模块。' }],
    ['meta', { property: 'og:image', content: `${SITE}logo.png` }],
    ['meta', { name: 'twitter:card', content: 'summary_large_image' }],

    ['link', { rel: 'alternate', hreflang: 'zh-CN', href: `${SITE}` }],
    ['link', { rel: 'alternate', hreflang: 'en-US', href: `${SITE}en/` }],
    ['link', { rel: 'alternate', hreflang: 'x-default', href: `${SITE}` }],

    // Cloudflare Web Analytics
    [
      'script',
      {
        type: 'module',
        src: 'https://static.cloudflareinsights.com/beacon.min.js',
        'data-cf-beacon': JSON.stringify({ token: '7a60b306ee8f46f38f2699681b26453e' })
      }
    ]
  ]
})
