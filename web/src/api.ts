import { createApi, fetchBaseQuery } from "@reduxjs/toolkit/query/react";
import type { CloudPlan, PlanInput, PlanUpdate } from "../../shared/plans";

export interface SessionData {
  user: { openId: string; name: string; avatarUrl?: string };
  csrfToken: string;
}

let csrfToken = "";

export const cloudApi = createApi({
  reducerPath: "cloudApi",
  baseQuery: fetchBaseQuery({
    baseUrl: "/api",
    credentials: "same-origin",
    prepareHeaders(headers) {
      if (csrfToken) headers.set("x-csrf-token", csrfToken);
      return headers;
    },
  }),
  tagTypes: ["Plans"],
  endpoints: (builder) => ({
    session: builder.query<SessionData, void>({
      query: () => "/session",
      transformResponse(response: SessionData) {
        csrfToken = response.csrfToken;
        return response;
      },
    }),
    plans: builder.query<CloudPlan[], void>({
      query: () => "/plans",
      transformResponse: (response: { plans: CloudPlan[] }) => response.plans,
      providesTags: ["Plans"],
    }),
    createPlan: builder.mutation<CloudPlan, PlanInput>({
      query: (body) => ({ url: "/plans", method: "POST", body }),
      transformResponse: (response: { plan: CloudPlan }) => response.plan,
      invalidatesTags: ["Plans"],
    }),
    updatePlan: builder.mutation<
      CloudPlan,
      { recordId: string; input: PlanUpdate }
    >({
      query: ({ recordId, input }) => ({
        url: `/plans/${recordId}`,
        method: "PATCH",
        body: input,
      }),
      transformResponse: (response: { plan: CloudPlan }) => response.plan,
      invalidatesTags: ["Plans"],
    }),
    deletePlan: builder.mutation<void, { recordId: string; version: number }>({
      query: ({ recordId, version }) => ({
        url: `/plans/${recordId}?version=${version}`,
        method: "DELETE",
      }),
      invalidatesTags: ["Plans"],
    }),
    logout: builder.mutation<void, void>({
      query: () => ({ url: "/logout", method: "POST" }),
    }),
  }),
});

export const {
  useSessionQuery,
  usePlansQuery,
  useCreatePlanMutation,
  useUpdatePlanMutation,
  useDeletePlanMutation,
  useLogoutMutation,
} = cloudApi;
