const body = document.getElementById("body");

const query = new URLSearchParams(window.location.search);
const name = query.get("v");

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
	body.innerHTML = "";
	body.appendChild(container(img));
}

function showVideo() {
	const video = document.createElement("video");
	video.src = `/${name}`;
	video.classList.add("full-screen");
	video.setAttribute("controls", "");
	body.innerHTML = "";
	body.appendChild(container(video));
}

function showText() {
	fetch(`/${name}`).then((resp) => {
		if (!checkStatus(resp.status)) return;

		resp.text().then((text) => {
			const p = document.createElement("p");
			p.innerHTML = escapeHtml(text);
			body.appendChild(p);
		});
	});
}

function showDownload(headers) {
	body.innerHTML = `No view for this file; download at <a href="/${name}">/${name}</a><br/>Content type: ${headers.get('Content-Type')}<br/>Size: ${headers.get("Content-Length")} bytes`;
}

function showError(msg) {
	body.innerHTML = msg;
}

function checkStatus(status) {
	if (status === 404) {
		showError(`<p>No media exists for id '${name}'</p>`);
	} else if (status === 500) {
		showError("Internal server error occurred... try refreshing");
	} else if (status !== 200) {
		showError("Unexpected error occurred... try refreshing");
	} else {
		return true;
	}

	return false;
}

export function showView() {
	if (name === null) {
		body.innerHTML = "Hi";
		return;
	}

	fetch(`/${name}`, { method: "HEAD" }).then((resp) => {
		if (!checkStatus(resp.status)) return;

		let type = resp.headers.get("Content-Type");
		if (type.startsWith("image")) {
			showImage();
		} else if (type.startsWith("video")) {
			showVideo();
		} else if (type.startsWith("text")) {
			showText();
		} else {
			showDownload(resp.headers);
		}
	});
}