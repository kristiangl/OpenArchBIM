// Printed line weights on the screen. A line prints at its pen width in mm; on the screen it is drawn at that width
// at 96 dpi whatever the zoom, since a line's weight doesn't change when the drawing is looked at closer. While a page
// is drawn for the PDF or DXF, `screen` is off and the width is the paper width at the page's scale.
const Pen = {
  PX_PER_MM: 96 / 25.4,
  screen: true,
  // the width of `mm` of pen; `k` is pixels per paper mm on a page
  w(mm, k) { return mm * (this.screen ? this.PX_PER_MM : k); },
};
