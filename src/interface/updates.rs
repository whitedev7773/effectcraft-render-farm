use super::*;

fn version_comparison(ui: &mut egui::Ui, current: &str, target: &str) {
    egui::Frame::new().fill(SURFACE_SECONDARY).corner_radius(8).inner_margin(layout::PADDING).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.columns(2, |columns| {
            caption(&mut columns[0], "이 PC의 버전");
            columns[0].label(egui::RichText::new(current.trim_start_matches('v')).size(25.0).strong());
            caption(&mut columns[1], "업데이트할 버전");
            columns[1].label(egui::RichText::new(target.trim_start_matches('v')).size(25.0).strong().color(egui::Color32::from_rgb(106, 181, 255)));
        });
    });
}

impl App {
    pub(crate) fn engine_update_ui(&mut self, ui: &mut egui::Ui) {
        if !self.engine_updater.prompt {
            return;
        }
        let Some(required) = self.engine_updater.required.clone() else { return };
        let idle = self.engine_update_idle();
        let installing = self.engine_updater.installing();
        let pending = self.engine_install_requested;
        let mut consent = false;
        let mut cancel = false;
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("effectcraft-update")).frame(card()).show(ui.ctx(), |ui| {
            ui.set_width((ui.ctx().content_rect().width() - 88.0).clamp(280.0, 460.0));
            close = dialog_header(ui, "EffectCraft 업데이트", "업데이트 창 닫기");
            caption(ui, "같은 버전으로 맞추면 함께 렌더링할 수 있습니다.");
            ui.add_space(8.0);
            egui::ScrollArea::vertical().id_salt("engine-update-body").max_height((ui.ctx().content_rect().height() - 260.0).clamp(150.0, 420.0)).show(
                ui,
                |ui| {
                    version_comparison(ui, &required.local, &required.target);
                    ui.add_space(layout::SPACE);
                    let both = required
                        .peer
                        .as_ref()
                        .zip(required.latest.as_ref())
                        .is_some_and(|(peer, latest)| effectcraft_update::higher(latest, peer).unwrap_or(false));
                    ui.label(if both {
                        "공식 최신 버전이 나왔습니다. 호스트와 작업 PC 모두 업데이트하세요."
                    } else if required.peer.is_some() {
                        "이 PC의 버전이 낮습니다. 상대 PC와 버전을 맞춰주세요."
                    } else {
                        "더 높은 공식 버전을 사용할 수 있습니다."
                    });
                    caption(ui, "공식 설치 파일을 자동으로 받아 설치 프로그램을 실행합니다.");
                    if pending {
                        ui.horizontal_wrapped(|ui| {
                            ui.spinner();
                            ui.label("진행 중인 작업을 마친 뒤 업데이트합니다.");
                        });
                    } else if installing {
                        ui.horizontal_wrapped(|ui| {
                            ui.spinner();
                            ui.label("다운로드 및 설치 진행 중");
                        });
                        caption(
                            ui,
                            format!(
                                "{:.1} MiB 수신 · 설치 창이 열리면 설치를 완료하세요.",
                                self.engine_updater.progress.load(Ordering::Relaxed) as f64 / 1048576.0
                            ),
                        );
                    } else if self.engine_updater.status.starts_with("EffectCraft 업데이트:")
                        || self.engine_updater.status.contains("실패")
                        || self.engine_updater.status.contains("취소")
                        || self.engine_updater.status.contains("재시작")
                    {
                        ui.colored_label(egui::Color32::LIGHT_RED, &self.engine_updater.status);
                    }
                    ui.collapsing("설치 및 버전 정보", |ui| {
                        if let Some(peer) = &required.peer {
                            caption(ui, format!("상대 PC · {peer}"));
                        }
                        if let Some(latest) = &required.latest {
                            caption(ui, format!("공식 릴리스 · {latest}"));
                        }
                        caption(ui, "공식 GitHub 파일의 크기와 SHA-256을 확인한 뒤 실행합니다. 설치가 완료되면 호스트에 다시 연결하세요.");
                        if !self.engine_updater.status.is_empty() {
                            caption(ui, &self.engine_updater.status);
                        }
                        ui.hyperlink_to("GitHub에서 릴리스 확인", installation::INSTALL_URL);
                    });
                    if !cfg!(windows) {
                        ui.hyperlink_to("공식 릴리스에서 설치", installation::INSTALL_URL);
                    }
                },
            );
            ui.add_space(8.0);
            ui.separator();
            if pending {
                let clicks = footer_buttons(ui, None, None, Some(ButtonSpec::new("업데이트 취소", ButtonKind::Normal, true)), layout::CONTROL);
                cancel = clicks.dismiss;
            } else if !installing {
                let clicks = footer_buttons(
                    ui,
                    None,
                    Some(ButtonSpec::new(
                        if idle { "다운로드 및 설치" } else { "연결 종료 후 설치" },
                        ButtonKind::Primary,
                        cfg!(windows) && !self.engine_updater.checking() && !self.updater.busy(),
                    )),
                    Some(ButtonSpec::new("나중에", ButtonKind::Normal, true)),
                    layout::CONTROL,
                );
                consent = clicks.primary;
                close |= clicks.dismiss;
            }
        });
        if response.should_close() || close {
            self.engine_updater.prompt = false;
        }
        if cancel {
            self.engine_install_requested = false;
        }
        if consent {
            self.request_engine_install();
        }
    }

    pub(crate) fn farm_update_ui(&mut self, ui: &mut egui::Ui, idle: bool) {
        let Some(release) = self.updater.available.clone() else { return };
        let mut consent = false;
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("update-consent")).frame(card()).show(ui.ctx(), |ui| {
            ui.set_width((ui.ctx().content_rect().width() - 88.0).clamp(280.0, 460.0));
            close = dialog_header(ui, "Render Farm 업데이트", "업데이트 창 닫기");
            caption(ui, "연결할 모든 PC의 Render Farm을 같은 버전으로 맞추세요.");
            ui.add_space(8.0);
            egui::ScrollArea::vertical().id_salt("farm-update-body").max_height((ui.ctx().content_rect().height() - 260.0).clamp(150.0, 420.0)).show(
                ui,
                |ui| {
                    version_comparison(ui, env!("CARGO_PKG_VERSION"), &release.tag_name);
                    ui.label("설치 파일을 받아 업데이트한 뒤 앱을 다시 실행합니다.");
                    if self.updater.status.starts_with("업데이트 실패:") {
                        ui.colored_label(egui::Color32::LIGHT_RED, &self.updater.status);
                    }
                    ui.collapsing("변경 내용", |ui| {
                        ui.label(release.body.as_deref().unwrap_or("등록된 변경 내용이 없습니다."));
                        ui.hyperlink_to("GitHub에서 릴리스 확인", &release.html_url);
                    });
                    if !idle {
                        caption(ui, "연결과 진행 중인 작업을 종료하면 업데이트할 수 있습니다.");
                    }
                },
            );
            ui.separator();
            let clicks = footer_buttons(
                ui,
                None,
                Some(ButtonSpec::new("업데이트 및 재시작", ButtonKind::Primary, idle && !self.updater.busy())),
                Some(ButtonSpec::new("나중에", ButtonKind::Normal, true)),
                layout::CONTROL,
            );
            consent = clicks.primary;
            close |= clicks.dismiss;
        });
        if response.should_close() || close {
            self.updater.prompt = false;
        }
        if consent {
            self.updater.consent_and_download();
        }
    }
}
