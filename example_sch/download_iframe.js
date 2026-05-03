const iframe = $x('//body//*[@id="ecad-viewer-div"]/iframe')[0];
const html = iframe.contentDocument.documentElement.outerHTML;
const blob = new Blob([html], {type: 'text/html'});
const a = document.createElement('a');
a.href = URL.createObjectURL(blob);
a.download = 'page.html';
a.click();
