import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

const githubEditBase = 'https://github.com/Lowband21/ferrex/edit/dev/docs/';

export default defineConfig({
  site: 'https://ferrexmedia.org/',
  base: '/',
  trailingSlash: 'always',
  integrations: [
    starlight({
      title: 'Ferrex Docs',
      description:
        'Documentation for operating, extending, and releasing the Ferrex media server and native clients.',
      tagline: 'Native media server and player documentation',
      logo: {
        src: './src/assets/ferrex-logo.svg',
        alt: 'Ferrex',
      },
      favicon: '/favicon.svg',
      social: [
        {
          icon: 'github',
          label: 'Ferrex on GitHub',
          href: 'https://github.com/Lowband21/ferrex',
        },
      ],
      editLink: {
        baseUrl: githubEditBase,
      },
      pagefind: true,
      tableOfContents: {
        minHeadingLevel: 2,
        maxHeadingLevel: 3,
      },
      customCss: ['./src/styles/ferrex.css'],
      head: [
        {
          tag: 'meta',
          attrs: {
            property: 'og:site_name',
            content: 'Ferrex Docs',
          },
        },
        {
          tag: 'meta',
          attrs: {
            name: 'theme-color',
            content: '#111827',
          },
        },
      ],
      sidebar: [
        { label: 'Overview', link: '/' },
        {
          label: 'Start here',
          items: [{ autogenerate: { directory: 'start' } }],
        },
        {
          label: 'Operate Ferrex',
          items: [{ autogenerate: { directory: 'operator' } }],
        },
        {
          label: 'Build and extend',
          items: [{ autogenerate: { directory: 'developer' } }],
        },
        {
          label: 'Reference',
          items: [{ autogenerate: { directory: 'reference' } }],
        },
        {
          label: 'Release',
          items: [{ autogenerate: { directory: 'release' } }],
        },
      ],
    }),
  ],
});
