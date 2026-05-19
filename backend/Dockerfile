FROM pyd4vinci/scrapling:latest

WORKDIR /app

# Copy project files
COPY pyproject.toml .
COPY app/ app/
COPY run.py .
COPY debug_check.py .

# Install dependencies via uv (already in scrapling base image)
RUN uv pip install --system fastapi "uvicorn[standard]" python-dotenv twitchio httpx

EXPOSE 9622

CMD ["uv", "run", "python", "run.py"]
