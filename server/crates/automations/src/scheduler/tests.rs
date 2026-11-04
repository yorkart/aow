use super::render::{unit_arg, xml};

#[test]
fn unit_arguments_and_xml_are_literal() {
    assert_eq!(unit_arg("/a %h/\"$x"), "\"/a %%h/\\\"$x\"");
    assert_eq!(xml("<&\"'>"), "&lt;&amp;&quot;&apos;&gt;");
}
