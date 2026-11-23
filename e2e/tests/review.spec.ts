// レビューの印（判定・見た物・レビュー待ち・上げた）を、本物の browser で見る。HTML の字は tests/ui.rs（Rust）が見る。
// 待ちの数は置き場の全部を数えるので、待ちにしたページは、試験の終わりまでに答えたにする（reply.spec.ts と同じ決まり）。
// 見出し「判定」は、判定を書いたページがあると出る。見出しの並びを見る list.spec.ts より後に流れるよう、この名にしてある。
import { expect, test } from '@playwright/test';
import {
  collectErrors,
  escalated,
  links,
  openLive,
  place,
  replyMark,
  replyOn,
  reviewMarks,
  reviewWaiting,
  rows,
  verdictMark,
  viewer,
  waiting,
} from './helpers';

const BAD_LIGHT = 'rgb(179, 38, 30)'; // --bad: #b3261e
const OK_LIGHT = 'rgb(31, 122, 77)'; // --new: #1f7a4d

test('レビュー: 頼むと依頼書にレビュー待ち、判定が返ると見た物と見られた側に印が出る。押さずに替わり、console に誤りが無い', async ({ page, request }) => {
  const errors = collectErrors(page);
  const to = { project: 'review', key: 'V-1', role: 'software-engineer' };
  const plan = await place(request, { ...to, kind: 'plan' }, '<title>計画</title>');
  const brief = await place(request, { ...to, kind: 'brief', role: 'software-engineer' }, '<title>レビューの依頼書</title>');
  const review = await place(request, { ...to, kind: 'review', role: 'staff-engineer' }, '<title>レビュー: 計画 plan-1</title>');
  try {
    await openLive(page, '/?project=review');
    await expect(rows(page)).toHaveCount(3);
    await expect(reviewWaiting(page)).toBeHidden();

    // 頼む: 依頼書に見る物とレビュー待ち。Product Manager の返事待ちは増えない。
    await reviewMarks(request, brief, { targets: [plan, 'https://github.com/o/r/pull/7'] });
    await replyOn(request, brief, 'waiting', 'review');
    await expect(replyMark(page, brief)).toHaveText('レビュー待ち');
    await expect(reviewWaiting(page)).toHaveText('レビュー待ち 1 件');
    await expect(waiting(page)).toBeHidden();
    await expect(page).toHaveTitle('ページの置き場');
    await expect(links(page, brief, 'targets').locator('a')).toHaveText(['plan-1', 'r#7']);
    await expect(links(page, brief, 'targets').locator('a').nth(1)).toHaveAttribute('target', '_blank');

    // 判定が返る: レビューの行に判定と見た物、計画の行に「レビュー:」。依頼書はレビュー済み。
    await reviewMarks(request, review, { verdict: 'approve', targets: [plan] });
    await replyOn(request, brief, 'answered', 'review');
    await expect(verdictMark(page, review)).toHaveText('よい');
    await expect(verdictMark(page, review)).toHaveCSS('color', OK_LIGHT);
    await expect(replyMark(page, brief)).toHaveText('レビュー済み');
    await expect(reviewWaiting(page)).toBeHidden();
    await expect(links(page, review, 'targets')).toHaveText('見た物: plan-1');
    await expect(links(page, plan, 'reviews')).toHaveText('レビュー: review-1よい');

    // 見た物の link は、一覧の中の本文の枠に開く。
    await links(page, plan, 'reviews').locator('a').click();
    await expect(viewer(page).locator('iframe')).toHaveAttribute('src', review);
    expect(errors).toEqual([]);
  } finally {
    await replyOn(request, brief, 'answered', 'review').catch(() => {});
  }
});

test('上げた: 上げると操作の帯に数と赤い印が出て、押すとそれだけに絞る。決まると消える', async ({ page, request }) => {
  const to = { project: 'review-up', key: 'U-1', role: 'staff-engineer' };
  const review = await place(request, { ...to, kind: 'review' }, '<title>レビュー: PR #9</title>');
  await reviewMarks(request, review, { verdict: 'reject', targets: ['https://github.com/o/r/pull/9'] });
  await replyOn(request, review, 'waiting', 'tpm');
  try {
    await openLive(page, '/?project=review-up');
    await expect(escalated(page)).toHaveText('上げた 1 件');
    await expect(escalated(page)).toHaveCSS('color', BAD_LIGHT);
    await expect(replyMark(page, review)).toHaveText('上げた');
    await expect(verdictMark(page, review)).toHaveText('直すべき');

    await escalated(page).click();
    await expect(page).toHaveURL(/[?&]reply=waiting&on=tpm/);
    await expect(rows(page)).toHaveCount(1);
    await expect(escalated(page)).toHaveAttribute('aria-current', 'true');

    await openLive(page, '/?project=review-up');
    await replyOn(request, review, 'answered', 'tpm');
    await expect(replyMark(page, review)).toHaveText('決まった');
    await expect(escalated(page)).toBeHidden();
  } finally {
    await replyOn(request, review, 'answered', 'tpm').catch(() => {});
  }
});
