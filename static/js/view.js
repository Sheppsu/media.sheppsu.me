const body = document.getElementById("body");

const query = new URLSearchParams(window.location.search);
const name = query.get("n");

function escapeHtml(text) {
    return text
         .replace(/&/g, "&amp;")
         .replace(/</g, "&lt;")
         .replace(/>/g, "&gt;")
         .replace(/"/g, "&quot;")
         .replace(/'/g, "&#039;");
}

function container(view) {
	const c = document.createElement("div");
	c.classList.add("view-container");
	c.appendChild(view);
	return c
}

function showImage() {
	const img = document.createElement("img");
	img.src = `/${name}`;
	img.classList.add("full-screen");
	body.appendChild(container(img));
}

function showVideo() {
	const video = document.createElement("video");
	video.src = `/${name}`;
	video.classList.add("full-screen");
	video.setAttribute("controls", "");
	body.appendChild(container(video));
}

function showText() {
	fetch(`/${name}`).then((resp) => {
		if (resp.ok) {
			resp.text().then((text) => {
				const p = document.createElement("p");
				p.innerHTML = escapeHtml(text);
				body.appendChild(p);
			});
		} else {
			showError();
		}
	});
}

function showDownload() {
	const p = document.createElement("p");
	p.innerHTML = `No view for this file, download at <a href="/${name}">/${name}</a>`
}

function showError() {
	const p = document.createElement("p");
	p.innerHTML = `Error trying to load a view for <a href="/${name}">/${name}</a>`;
	body.appendChild(p);
}

export function showView(mime) {
	console.log(mime);
	if (mime.includes("image")) {
		showImage();
	} else if (mime.includes("video")) {
		showVideo();
	} else if (mime.includes("text")) {
		showText();
	} else {
		showDownload();
	}
}