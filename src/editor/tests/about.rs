use super::*;

#[test]
fn about_links_and_contributor_rows_use_the_requested_destinations() {
    let mut f = Fixture::new();
    f.editor.about_open = true;
    f.frame(vec![]);
    f.frame(vec![]);
    for label in ["GitHub", "Bilibili", "X"] {
        f.click_text(label);
    }
    assert_eq!(
        f.opened_urls,
        vec![
            "https://github.com/sout233",
            "https://space.bilibili.com/230275560",
            "https://x.com/sout_Nantang",
        ]
    );

    f.click_text("鸣谢");
    f.frame(vec![]);
    for (name, destination) in [
        (
            "Icewatermelon233（谭荣熙竣）",
            "https://space.bilibili.com/393632577",
        ),
        ("UnD3ath", "https://space.bilibili.com/224632474"),
    ] {
        let bounds = f
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.job.text == name => {
                    Some(shape.shape.visual_bounding_rect())
                }
                _ => None,
            })
            .unwrap();
        let link = f
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t)
                    if t.galley.job.text == "Bilibili"
                        && (shape.shape.visual_bounding_rect().center().y - bounds.center().y)
                            .abs()
                            < 5.0 =>
                {
                    Some(shape.shape.visual_bounding_rect())
                }
                _ => None,
            })
            .unwrap();
        assert!(link.left() > bounds.right());
        f.click(link.center(), egui::PointerButton::Primary);
        assert_eq!(f.opened_urls.last().unwrap(), destination);
    }
    assert!(f.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(t) if t.galley.job.text == "荔枝fliz")));
    assert!(f.host.events.lock().unwrap().is_empty());
}
