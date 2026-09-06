import { configureStore } from "@reduxjs/toolkit";
import {
  Provider,
  useDispatch,
  useSelector,
  type TypedUseSelectorHook,
} from "react-redux";
import { cloudApi } from "./api";

export const store = configureStore({
  reducer: { [cloudApi.reducerPath]: cloudApi.reducer },
  middleware: (getDefaultMiddleware) =>
    getDefaultMiddleware().concat(cloudApi.middleware),
});

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;
export const useAppDispatch = () => useDispatch<AppDispatch>();
export const useAppSelector: TypedUseSelectorHook<RootState> = useSelector;
export { Provider };
