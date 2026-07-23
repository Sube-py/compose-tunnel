import { createApp } from "vue";
import PrimeVue from "primevue/config";
import ConfirmationService from "primevue/confirmationservice";
import ToastService from "primevue/toastservice";
import Tooltip from "primevue/tooltip";
import Aura from "@primeuix/themes/aura";
import App from "./App.vue";
import "primeicons/primeicons.css";
import "./styles/main.css";

createApp(App)
  .use(PrimeVue, {
    theme: {
      preset: Aura,
      options: {
        darkModeSelector: false,
      },
    },
  })
  .use(ConfirmationService)
  .use(ToastService)
  .directive("tooltip", Tooltip)
  .mount("#app");
