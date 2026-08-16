# -*- coding: utf-8 -*-
# 一次性生成 plan draft fixture:独立实现旧 TS 本地算法(表征测试基准)。
import datetime
import json


def iso(d):
    return d.isoformat()


def build(exam_date, start, hours):
    start = datetime.date.fromisoformat(start)
    exam = datetime.date.fromisoformat(exam_date)
    total_days = (exam - start).days
    assert total_days > 0
    base = max(int(total_days * 0.5), 1)
    stren = max(int(total_days * 0.3), 1)
    sprint = max(total_days - base - stren, 1)
    subjects = [("sub-math", "数学", 2.0, 3), ("sub-eng", "英语", 1.0, 4)]
    kps = {
        "sub-math": [("kp-func", "函数", 2), ("kp-geo", "几何", 4)],
        "sub-eng": [("kp-word", "词汇", 3)],
    }
    gaps = [max(6 - lvl, 0) * w for _, _, w, lvl in subjects]
    total_gap = max(sum(gaps), 1.0)
    kp_idx = {"sub-math": 0, "sub-eng": 0}
    days = []
    for off in range(total_days):
        d = start + datetime.timedelta(days=off)
        epoch = (d - datetime.date(1970, 1, 1)).days
        sunday = (epoch + 4) % 7 == 0
        total_min = int(round((hours / 2 if sunday else hours) * 60))
        tasks = []
        for i, (sid, name, w, lvl) in enumerate(subjects):
            raw = total_min * gaps[i] / total_gap
            m = int(round(raw))
            if m < 10:
                continue
            kps_list = kps[sid]
            kid = None
            task = "{name} 综合复习"
            if kps_list:
                idx = kp_idx[sid] % len(kps_list)
                kid, kname, _ = kps_list[idx]
                task = ("复习：" + kname) if sunday else ("学习：" + kname)
                kp_idx[sid] = idx + 1
            tasks.append(
                {
                    "subject_id": sid,
                    "subject_name": name,
                    "knowledge_point_id": kid,
                    "task": task,
                    "duration_min": m,
                }
            )
        days.append({"date": iso(d), "tasks": tasks})
    total_tasks = sum(len(d["tasks"]) for d in days)
    total_min = sum(t["duration_min"] for d in days for t in d["tasks"])
    return {
        "input": {
            "exam_date": exam_date,
            "start_date": iso(start),
            "daily_hours": hours,
        },
        "total_days": total_days,
        "phases": [
            {
                "name": "基础期",
                "start": iso(start),
                "end": iso(start + datetime.timedelta(days=base - 1)),
            },
            {
                "name": "强化期",
                "start": iso(start + datetime.timedelta(days=base)),
                "end": iso(start + datetime.timedelta(days=base + stren - 1)),
            },
            {
                "name": "冲刺期",
                "start": iso(start + datetime.timedelta(days=base + stren)),
                "end": iso(start + datetime.timedelta(days=total_days - 1)),
            },
        ],
        "daily_plans": days,
        "summary": {
            "total_tasks": total_tasks,
            "total_duration_min": total_min,
            "avg_daily_min": total_min // total_days,
        },
    }


def main():
    r = build("2030-01-31", "2030-01-01", 6.0)
    with open("tests/fixtures/plan-draft-rotation-30d.json", "w", encoding="utf-8") as f:
        json.dump(r, f, ensure_ascii=False, indent=1)
    print("days", r["total_days"], "tasks", r["summary"]["total_tasks"], "total_min", r["summary"]["total_duration_min"])
    for d in r["daily_plans"][:3] + r["daily_plans"][5:6] + r["daily_plans"][-1:]:
        print(d["date"], [(t["subject_id"], t["duration_min"], t["task"], t["knowledge_point_id"]) for t in d["tasks"]])


if __name__ == "__main__":
    main()
