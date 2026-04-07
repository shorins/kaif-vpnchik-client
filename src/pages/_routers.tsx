import HomeRoundedIcon from '@mui/icons-material/HomeRounded'
import { createBrowserRouter, RouteObject } from 'react-router'

import HomeSvg from '@/assets/image/itemicon/home.svg?react'

import Layout from './_layout'
import HomePage from './home'

export const navItems = [
  {
    label: 'layout.components.navigation.tabs.home',
    path: '/',
    icon: [<HomeRoundedIcon key="mui" />, <HomeSvg key="svg" />],
    Component: HomePage,
  },
]

export const router = createBrowserRouter([
  {
    path: '/',
    Component: Layout,
    children: navItems.map(
      (item) =>
        ({
          path: item.path,
          Component: item.Component,
        }) as RouteObject,
    ),
  },
])
