// Copy to clipboard functionality
document.addEventListener('DOMContentLoaded', function() {
    const copyButtons = document.querySelectorAll('.copy-btn');

    copyButtons.forEach(function(button) {
        button.addEventListener('click', function() {
            const targetId = this.getAttribute('data-target');
            const codeElement = document.getElementById(targetId);

            if (codeElement) {
                const text = codeElement.textContent;

                navigator.clipboard.writeText(text).then(function() {
                    button.textContent = 'Copied!';
                    button.classList.add('copied');

                    setTimeout(function() {
                        button.textContent = 'Copy';
                        button.classList.remove('copied');
                    }, 2000);
                }).catch(function(err) {
                    console.error('Failed to copy:', err);
                });
            }
        });
    });
});
