import { z } from "zod";

export const planStatusSchema = z.enum([
  "needs_clarification",
  "scheduled",
  "done",
]);

const httpUrl = z
  .string()
  .url()
  .refine((value) => /^https?:\/\//i.test(value), "链接只允许 http 或 https");

export const planInputSchema = z.object({
  title: z.string().trim().min(1).max(80),
  details: z.string().trim().min(1).max(4_000),
  content: z.string().trim().max(60).default(""),
  linkUrl: z.union([httpUrl, z.literal("")]).optional(),
  notes: z.string().trim().max(500).default(""),
  scheduledAt: z.number().int().min(946684800000).max(4102444800000).nullable(),
  status: planStatusSchema,
  reminderMinutesBefore: z.number().int().min(0).max(10080).default(180),
  tags: z.array(z.literal("面试")).max(1).default([]),
  sourceTitle: z.string().trim().max(500).default("网页创建"),
});

export const planUpdateSchema = z.object({
  title: z.string().trim().min(1).max(80).optional(),
  details: z.string().trim().min(1).max(4_000).optional(),
  content: z.string().trim().max(60).optional(),
  linkUrl: z.union([httpUrl, z.literal("")]).optional(),
  notes: z.string().trim().max(500).optional(),
  scheduledAt: z
    .number()
    .int()
    .min(946684800000)
    .max(4102444800000)
    .nullable()
    .optional(),
  status: planStatusSchema.optional(),
  reminderMinutesBefore: z.number().int().min(0).max(10080).optional(),
  tags: z.array(z.literal("面试")).max(1).optional(),
  sourceTitle: z.string().trim().max(500).optional(),
  version: z.number().int().nonnegative(),
});

export interface CloudPlan {
  recordId: string;
  id: string;
  title: string;
  details: string;
  content: string;
  linkUrl?: string;
  notes?: string;
  scheduledAt?: number;
  status: z.infer<typeof planStatusSchema>;
  sourceTitle: string;
  updatedAt: number;
  reminderMinutesBefore: number;
  tags: string[];
  version: number;
}

export type PlanInput = z.infer<typeof planInputSchema>;
export type PlanUpdate = z.infer<typeof planUpdateSchema>;
