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
    ['meta', { property: 'og:image', content: `${SITE}og-image.png` }],
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
    ],
    // 语言分流：有记忆时按记忆语言主动跳转对应版本；无记忆时非中文浏览器自动跳英文
    [
      'script',
      {},
      `      ;(() => {
        const base = '/device_faker/'
        const path = location.pathname
        if (!path.startsWith(base)) return
        const sub = path.slice(base.length)
        const cur = sub.startsWith('en/') || sub === 'en'
        let pref = null
        try { pref = localStorage.getItem('lang-pref:/device_faker/') } catch (e) {}
        if (pref === 'zh' || pref === 'en') {
          if (pref === 'en' && !cur) {
            location.replace(base + 'en/' + sub + location.search + location.hash)
          } else if (pref === 'zh' && cur) {
            let rest = sub === 'en' ? '' : sub.slice(3)
            location.replace(base + rest + location.search + location.hash)
          }
          return
        }
        if (!cur && !(navigator.language || '').toLowerCase().startsWith('zh')) {
          location.replace(base + 'en/' + sub + location.search + location.hash)
        }
      })()
            ;(() => {
        const base = '/device_faker/'
        const onPick = e => {
          const a = e.target && e.target.closest ? e.target.closest('a') : null
          if (!a) return
          const href = a.getAttribute('href') || ''
          let sub = null
          if (href.startsWith(base)) sub = href.slice(base.length)
          else {
            try {
              const u = new URL(href, location.origin)
              if (u.origin === location.origin && u.pathname.startsWith(base)) sub = u.pathname.slice(base.length)
            } catch (err) {}
          }
          if (sub === null) return
          const cur = location.pathname.slice(base.length)
          const t = sub.startsWith('en/') || sub === 'en'
          const c = cur.startsWith('en/') || cur === 'en'
          if (t !== c) {
            const lang = t ? 'en' : 'zh'
            const auto = (navigator.language || '').toLowerCase().startsWith('zh') ? 'zh' : 'en'
            try {
              if (lang === auto) localStorage.removeItem('lang-pref:/device_faker/')
              else localStorage.setItem('lang-pref:/device_faker/', lang)
            } catch (err) {}
          }
        }
        // capture 阶段 + pointerdown 双保险：早于扩展对 DOM/事件的包装
        document.addEventListener('click', onPick, true)
        document.addEventListener('pointerdown', onPick, true)
      })()`
    ]
  ]
})
