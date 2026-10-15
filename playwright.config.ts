import {defineConfig} from '@playwright/test';
export default defineConfig({testDir:'./tests/e2e',timeout:90000,workers:1,use:{baseURL:process.env.STORYFOLD_TEST_URL??'http://127.0.0.1:5173',viewport:{width:1440,height:1000},launchOptions:process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE?{executablePath:process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE}:{}},reporter:'list'});
