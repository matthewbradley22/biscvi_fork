export function color_gradient(ctx) {
const gradient = ctx.createLinearGradient(0, 0, 0, 180);

// Add three color stops
gradient.addColorStop(0, "red");
gradient.addColorStop(1, "black");

// Set the fill style and draw a rectangle
ctx.fillStyle = gradient;
ctx.fillRect(5, 0, 15, 180);
return(ctx);
}